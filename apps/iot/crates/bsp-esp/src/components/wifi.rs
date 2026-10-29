//! The Wi-Fi station on top of the radio driver: what a scan keeps, how a connection
//! attempt is made to fail, and what "connected" means.
//!
//! Connect by scanning every channel and taking the strongest AP, retrying it three times
//! before falling back. A saved hint names one AP and one channel and skips the walk.
//! `[实测]` the driver's fast scan connected at −79 dBm while a −73 dBm AP of the same name
//! was in range. A listen interval of 10 beacons trades latency for power.

use alloc::{boxed::Box, vec::Vec};
use esp_radio::wifi::{
    AuthenticationMethod, AuthenticationMethodConfig, Config, ControllerConfig, DisconnectReason,
    Interface, Password, Ssid, WifiController, WifiError,
    ap::{AccessPointConfig, AccessPointInfo},
    scan::{ScanConfig, ScanTypeConfig},
    sta::{DisconnectedInfo, ScanMethod, StationConfig},
};
use iot_core::drivers::network::{
    AccessPoint, Credentials, Ipv4, LinkHint, LinkPhase, LinkStatus, MAX_LISTED_APS, NetworkError,
    SetupPass, Ssid as NetSsid, Station, rank_aps,
};
use iot_core::provision::SETUP_PASS_LEN;

/// The station's frame interface, handed up to the IP stack.
pub type EspInterface = Interface;

const SCAN_REQUESTS: usize = MAX_LISTED_APS * 2;

const STATIC_RX_BUFFERS: u8 = 3;
const DYNAMIC_RX_BUFFERS: u16 = 6;
const BLOCK_ACK_WINDOW: u8 = 3;

const STRONGEST_RETRIES: u8 = 3;

const LISTEN_INTERVAL: u16 = 10;

// How long the radio sits on each channel while scanning it.
//
// **Do not drop this back to the driver's 10–20 ms default.** A sweep at the default
// finishes in 135–160 ms and reports one or two access points on this bench; at 50–120 ms
// it takes ~0.8 s and reports three to six. The cost is a second of a user's wait, the
// saving is the difference between a list and a list of the two loudest networks.
// `@/records/iot/wifi.md`.
const CHANNEL_DWELL: ScanTypeConfig = ScanTypeConfig::Active {
    min: esp_hal::time::Duration::from_millis(50),
    max: esp_hal::time::Duration::from_millis(120),
};

/// A station on this chip's radio.
pub struct EspStation {
    controller: WifiController<'static>,
    device: Option<EspInterface>,
    status: LinkStatus,
    setup_pass: SetupPass,
}

// What the setup passphrase is made of: digits and letters with no pair a person mixes up — no
// `0`/`o`, no `1`/`l` — so it can be copied off a 320 px row and typed into a phone's own keypad
// without a second look.
const SETUP_PASS_ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstuvwxyz";

impl EspStation {
    /// Bring the radio up and hand back a station in STA mode.
    pub fn new(wifi: esp_hal::peripherals::WIFI<'static>) -> Result<Self, NetworkError> {
        let interface = Interface::try_station().ok_or_else(|| {
            log::error!("[WIFI] the station interface is already taken");
            NetworkError::Driver
        })?;
        let controller = WifiController::new(
            wifi,
            ControllerConfig::default()
                .with_static_rx_buf_num(STATIC_RX_BUFFERS)
                .with_dynamic_rx_buf_num(DYNAMIC_RX_BUFFERS)
                .with_rx_ba_win(BLOCK_ACK_WINDOW),
        )
        .map_err(Self::kind_of)?;

        Ok(Self {
            controller,
            device: Some(interface),
            status: LinkStatus::default(),
            setup_pass: Self::make_setup_pass(),
        })
    }

    fn make_setup_pass() -> SetupPass {
        let rng = esp_hal::rng::Rng::new();
        let mut text = [0u8; SETUP_PASS_LEN];
        for slot in &mut text {
            *slot = SETUP_PASS_ALPHABET[rng.random() as usize % SETUP_PASS_ALPHABET.len()];
        }
        SetupPass::from_array(text)
    }

    fn kind_of(error: WifiError) -> NetworkError {
        let kind = match error {
            WifiError::OutOfMemory => NetworkError::NoMemory,
            WifiError::InvalidArguments
            | WifiError::InvalidSsid
            | WifiError::InvalidPassword
            | WifiError::Unsupported => NetworkError::InvalidArguments,
            // The error set is `non_exhaustive`, and an arm that names the ones
            // this build knows is the point: a new one is a gap the compiler
            // reports rather than a kind silently invented here.
            _ => NetworkError::Driver,
        };
        log::error!("[WIFI] driver refused: {error:?} ({kind:?})");
        kind
    }

    fn kind_of_disconnect(disconnected: &DisconnectedInfo) -> NetworkError {
        log::error!(
            "[WIFI] left {:?}: {:?} at {} dBm",
            disconnected.ssid.as_str(),
            disconnected.reason,
            disconnected.rssi
        );
        // Three of the driver's reasons become kinds of their own, because they are the three a
        // user standing at the panel can do something about, and they are told apart in the log by
        // the driver's own word for them either way. The rest stay `Driver` rather than being
        // guessed at: the reason set is long, and a wrong guess would put a confident word on the
        // panel about a failure nobody diagnosed.
        match disconnected.reason {
            DisconnectReason::NoAccessPointFound | DisconnectReason::BeaconTimeout => {
                NetworkError::NotFound
            }
            DisconnectReason::AuthenticationFailed
            | DisconnectReason::FourWayHandshakeTimeout
            | DisconnectReason::HandshakeTimeout => NetworkError::Credentials,
            _ => NetworkError::Driver,
        }
    }

    fn row(record: &AccessPointInfo) -> AccessPoint {
        AccessPoint {
            ssid: NetSsid::from_octets(record.ssid.as_str().as_bytes()),
            bssid: record.bssid,
            channel: record.channel,
            rssi: record.signal_strength,
            secured: record
                .auth_method
                .is_some_and(|method| method != AuthenticationMethod::None),
        }
    }

    fn config_for(credentials: &Credentials, hint: LinkHint) -> Result<Config, NetworkError> {
        let ssid = Ssid::try_from(credentials.ssid.as_bytes()).map_err(Self::kind_of)?;
        let auth = if credentials.password.is_empty() {
            AuthenticationMethodConfig::Open
        } else {
            AuthenticationMethodConfig::Wpa2Personal(
                Password::try_from(credentials.password.as_bytes()).map_err(Self::kind_of)?,
            )
        };
        let known = hint.is_known();
        let mut station = StationConfig::default()
            .with_ssid(ssid)
            .with_authentication(auth)
            .with_scan_method(if known {
                ScanMethod::Fast
            } else {
                ScanMethod::AllChannels
            })
            .with_failure_retry_cnt(STRONGEST_RETRIES)
            .with_listen_interval(LISTEN_INTERVAL);
        if known {
            station = station.with_bssid(hint.bssid).with_channel(hint.channel);
        }
        Ok(Config::Station(station))
    }
}

impl Station for EspStation {
    type Device = EspInterface;

    async fn scan(&mut self) -> Result<Vec<AccessPoint>, NetworkError> {
        self.status.phase = LinkPhase::Scanning;
        let config = ScanConfig::default()
            .with_max(SCAN_REQUESTS)
            .with_scan_type(CHANNEL_DWELL);
        let found = self.controller.scan_async(&config).await;
        self.status.phase = LinkPhase::Off;
        let found = found.map_err(Self::kind_of)?;

        let rows = found.iter().map(Self::row).collect::<Vec<_>>();
        let ranked = rank_aps(&rows, MAX_LISTED_APS);
        for row in &ranked {
            log::debug!(
                "[WIFI] {:>4} dBm {} bars  channel {}  {}",
                row.rssi,
                row.bars(),
                row.channel,
                row.ssid.as_display_text().unwrap_or("<not drawable>")
            );
        }
        Ok(ranked)
    }

    fn begin_connect(&mut self, credentials: &Credentials) -> Result<(), NetworkError> {
        self.begin_connect_hinted(credentials, LinkHint::NONE)
    }

    fn begin_connect_hinted(
        &mut self,
        credentials: &Credentials,
        hint: LinkHint,
    ) -> Result<(), NetworkError> {
        let refusal = match Self::config_for(credentials, hint) {
            Ok(config) => self.controller.set_config(&config).err().map(Self::kind_of),
            Err(kind) => Some(kind),
        };
        if let Some(kind) = refusal {
            self.status.phase = LinkPhase::Failed(kind);
            return Err(kind);
        }

        self.status = self.status.with_address(None);
        self.status.phase = LinkPhase::Connecting;
        Ok(())
    }

    async fn await_connected(&mut self) -> Result<(), NetworkError> {
        match self.controller.connect_async().await {
            Ok(connected) => {
                self.status.ap = Some(AccessPoint {
                    ssid: NetSsid::from_octets(connected.ssid.as_str().as_bytes()),
                    bssid: connected.bssid,
                    channel: connected.channel,
                    rssi: 0,
                    secured: connected.authmode != AuthenticationMethod::None,
                });
                log::info!(
                    "[WIFI] associated with {} on channel {}",
                    connected.ssid.as_str(),
                    connected.channel
                );
                Ok(())
            }
            Err(esp_radio::wifi::ConnectionError::Failed(disconnected)) => {
                let kind = Self::kind_of_disconnect(&disconnected);
                self.status.phase = LinkPhase::Failed(kind);
                Err(kind)
            }
            Err(esp_radio::wifi::ConnectionError::WifiError(error)) => {
                let kind = Self::kind_of(error);
                self.status.phase = LinkPhase::Failed(kind);
                Err(kind)
            }
            Err(other) => {
                log::error!("[WIFI] association gave up: {other:?}");
                self.status.phase = LinkPhase::Failed(NetworkError::Driver);
                Err(NetworkError::Driver)
            }
        }
    }

    async fn disconnect(&mut self) -> Result<(), NetworkError> {
        self.controller
            .disconnect_async()
            .await
            .map_err(Self::kind_of)?;
        self.status = LinkStatus::default();
        Ok(())
    }

    fn status(&self) -> LinkStatus {
        self.status
    }

    fn note_address(&mut self, address: Option<Ipv4>) {
        self.status = self.status.with_address(address);
    }

    fn take_device(&mut self) -> Option<Self::Device> {
        self.device.take()
    }

    fn begin_setup(&mut self, ssid: &NetSsid, secured: bool) -> Result<Self::Device, NetworkError> {
        let interface = Interface::try_access_point().ok_or_else(|| {
            log::error!("[WIFI] the access-point interface is already taken");
            NetworkError::Driver
        })?;
        let name = Ssid::try_from(ssid.as_bytes()).map_err(Self::kind_of)?;
        let mut access_point = AccessPointConfig::default().with_ssid(name);
        if secured {
            let password = Password::try_from(self.setup_pass.as_bytes()).map_err(Self::kind_of)?;
            access_point = access_point
                .with_authentication(AuthenticationMethodConfig::Wpa2Personal(password));
        }
        let config = Config::AccessPointStation(StationConfig::default(), access_point);
        self.controller.set_config(&config).map_err(Self::kind_of)?;
        self.status = LinkStatus::default();
        self.status.setup_password = secured.then_some(self.setup_pass);
        Ok(interface)
    }

    fn end_setup(&mut self) -> Result<(), NetworkError> {
        self.controller
            .set_config(&Config::Station(StationConfig::default()))
            .map_err(Self::kind_of)?;
        self.status = LinkStatus::default();
        Ok(())
    }
}

/// Start a radio a board took off its bring-up, or say in the log why it would not start.
pub fn start(radio: esp_hal::peripherals::WIFI<'static>) -> Option<Box<EspStation>> {
    match EspStation::new(radio) {
        Ok(station) => Some(Box::new(station)),
        Err(error) => {
            log::error!("[WIFI] the radio would not start: {error:?}");
            None
        }
    }
}
