//! The Wi-Fi station, on top of the radio driver.
//!
//! The driver is a closed blob behind a fairly wide API, and this is where the
//! product's choices meet it: what a scan keeps, how a connection attempt is made
//! to fail, and what "connected" means. Each is a decision rather than a default,
//! so it is written next to the call it governs.
//!
//! Two are copied from the reference firmware on this same module, because they
//! answer questions the driver's own defaults answer badly:
//!
//! * **Connect by scanning every channel and taking the strongest access point,
//!   retrying the strongest three times before falling back to a weaker one of the
//!   same name.** The default is a fast scan that takes the first match, so a
//!   strong AP that rejects the first authentication attempt is silently replaced
//!   by a weak one — measured there as connecting to −79 dBm while a −73 dBm AP of
//!   the same name was in range.
//! * **A listen interval of 10 beacons.** The AP may skip us for that many beacon
//!   intervals between polls, which buys power at the cost of latency; a voice
//!   device pays less of the second than a download client would.
//!
//! The rest is refusal or naming. The driver's errors are logged where they happen
//! and reported upward as a [`NetworkError`] kind, because the panel acts on the
//! kind and the code belongs in the log next to the call that produced it.

use alloc::{boxed::Box, vec::Vec};
use esp_radio::wifi::{
    AuthenticationMethod, AuthenticationMethodConfig, Config, ControllerConfig, DisconnectReason,
    Interface, Password, Ssid, WifiController, WifiError,
    ap::AccessPointInfo,
    scan::{ScanConfig, ScanTypeConfig},
    sta::{DisconnectedInfo, ScanMethod, StationConfig},
};
use iot_core::drivers::network::{
    AccessPoint, Credentials, Ipv4, LinkPhase, LinkStatus, MAX_LISTED_APS, NetworkError,
    Ssid as NetSsid, Station, rank_aps,
};

/// The station's frame interface, handed up to the IP stack.
///
/// The driver's own type rather than a newtype: the IP stack's driver trait is
/// implemented for it, and a wrapper would mean forwarding that trait by hand to
/// reach a device that already answers it.
pub type EspInterface = Interface;

/// How many access points to ask the driver for.
///
/// Above [`MAX_LISTED_APS`] on purpose: the ranking drops the weakest, so asking
/// for the panel's eight and keeping the driver's first eight would be asking the
/// driver's ordering to be the ranking.
const SCAN_REQUESTS: usize = MAX_LISTED_APS * 2;

/// Receive buffers the driver keeps for the radio's lifetime, and the block-ack
/// window they serve.
///
/// The driver's default is 10 static buffers of about 1.6 KB each, ~16 KB this part
/// does not have — the reference firmware runs 3 static and 6 dynamic with the
/// window at 3. What those numbers buy is throughput, and a link carrying a screen
/// and a microphone is not where that shows.
const STATIC_RX_BUFFERS: u8 = 3;
const DYNAMIC_RX_BUFFERS: u16 = 6;
const BLOCK_ACK_WINDOW: u8 = 3;

/// Attempts on the strongest access point before falling back to a weaker one of
/// the same name. See the module docs.
const STRONGEST_RETRIES: u8 = 3;

/// Listen interval in beacon intervals. See the module docs.
const LISTEN_INTERVAL: u16 = 10;

/// How long the radio sits on each channel while scanning it.
///
/// **Do not drop this back to the driver's 10–20 ms default.** A sweep at the default
/// finishes in 135–160 ms and reports one or two access points on this bench; at 50–120 ms
/// it takes ~0.8 s and reports three to six. The cost is a second of a user's wait, the
/// saving is the difference between a list and a list of the two loudest networks.
/// `@/records/iot/wifi.md`.
const CHANNEL_DWELL: ScanTypeConfig = ScanTypeConfig::Active {
    min: esp_hal::time::Duration::from_millis(50),
    max: esp_hal::time::Duration::from_millis(120),
};

/// A station on this chip's radio.
pub struct EspStation {
    controller: WifiController<'static>,
    /// Handed to the IP stack, which owns it from then on: the stack polls frames
    /// and this is what changes the radio's mode, so one borrow cannot be both.
    device: Option<EspInterface>,
    status: LinkStatus,
}

impl EspStation {
    /// Bring the radio up and hand back a station in STA mode.
    ///
    /// The scheduler must already be running: the driver starts its own tasks and
    /// allocates from the heap [`iot_chip_esp::init_radio_heap`] sets up, which is
    /// why the board hands the peripheral over rather than building this inside its
    /// own bring-up.
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
        })
    }

    /// Log the driver's own error where it happened and return the kind upward.
    ///
    /// `OutOfMemory` is named rather than folded into "the radio said no": it is
    /// the difference between a wrong passphrase and a build whose memory budget
    /// is wrong, which look identical from the panel otherwise.
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

    /// One driver's record as a row the panel can rank.
    ///
    /// The name comes back through `as_str`, which stops at the first octet that is
    /// not valid UTF-8 rather than failing; a name in a non-UTF-8 encoding is
    /// therefore read back truncated in the log while the row itself is ranked on
    /// its BSSID, which is what identifies it anyway.
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

    /// The driver's own config for one credential, as far as the driver's own validation
    /// takes it.
    ///
    /// Built through the driver's types rather than this crate's so that the limits the radio
    /// enforces are the limits a credential is refused by: a second opinion here would be a
    /// passphrase this crate thinks is short and the radio does not.
    fn config_for(credentials: &Credentials) -> Result<Config, NetworkError> {
        let ssid = Ssid::try_from(credentials.ssid.as_bytes()).map_err(Self::kind_of)?;
        // An empty passphrase is the network the list marked `OPEN`: the driver has to be told
        // there is nothing to ask for, because a WPA2 config carrying no password is a request the
        // access point refuses rather than one it ignores. The list's own `OPEN` marker and this
        // reading of the credential are the same fact, and the credential is the one that reaches
        // the radio.
        let auth = if credentials.password.is_empty() {
            AuthenticationMethodConfig::Open
        } else {
            AuthenticationMethodConfig::Wpa2Personal(
                Password::try_from(credentials.password.as_bytes()).map_err(Self::kind_of)?,
            )
        };
        let station = StationConfig::default()
            .with_ssid(ssid)
            .with_authentication(auth)
            .with_scan_method(ScanMethod::AllChannels)
            .with_failure_retry_cnt(STRONGEST_RETRIES)
            .with_listen_interval(LISTEN_INTERVAL);
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
            log::info!(
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
        // A credential the driver will not take is refused here rather than in the wait, and the
        // phase is where it is reported: the panel reads refusals off the phase, and one that only
        // the log ever saw would leave a name or a passphrase the radio is holding out for looking
        // like nothing happening at all.
        let refusal = match Self::config_for(credentials) {
            Ok(config) => self.controller.set_config(&config).err().map(Self::kind_of),
            Err(kind) => Some(kind),
        };
        if let Some(kind) = refusal {
            self.status.phase = LinkPhase::Failed(kind);
            return Err(kind);
        }

        // A new attempt invalidates the old address: it belonged to the previous link,
        // and leaving it in the status would let a report claim an address this
        // association has not earned yet.
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
                    // The association report carries no signal reading. Leaving it at
                    // zero rather than borrowing one from an earlier scan: a stale
                    // number would be a measurement, and this is a gap.
                    rssi: 0,
                    secured: connected.authmode != AuthenticationMethod::None,
                });
                log::info!(
                    "[WIFI] associated with {:?} on channel {}",
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
            // `non_exhaustive` again: a new outcome is a gap to notice rather than a
            // kind to invent, so it lands on the same one as every other refusal.
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
}

/// Start a radio a board took off its bring-up, or say in the log why it would not start.
///
/// Builds the station rather than handing one over, because the driver's start needs the
/// scheduler and every board calls this after it. That shape is the same on every board that
/// carries a radio, which is what lets a caller ask for a station without naming the part
/// underneath it.
pub fn start(radio: esp_hal::peripherals::WIFI<'static>) -> Option<Box<EspStation>> {
    match EspStation::new(radio) {
        Ok(station) => Some(Box::new(station)),
        Err(error) => {
            log::error!("[WIFI] the radio would not start: {error:?}");
            None
        }
    }
}
