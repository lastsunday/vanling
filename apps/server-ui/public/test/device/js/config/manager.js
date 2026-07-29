let otaToken = '';
let activationCode = '';

function generateRandomMac() {
    const hexDigits = '0123456789ABCDEF';
    let mac = '';
    for (let i = 0; i < 6; i++) {
        if (i > 0) mac += ':';
        for (let j = 0; j < 2; j++) {
            mac += hexDigits.charAt(Math.floor(Math.random() * 16));
        }
    }
    return mac;
}

export function loadConfig() {
    const deviceMacInput = document.getElementById('deviceMac');
    const deviceNameInput = document.getElementById('deviceName');
    const clientIdInput = document.getElementById('clientId');
    const otaUrlInput = document.getElementById('otaUrl');

    let savedMac = localStorage.getItem('xz_tester_deviceMac');
    if (!savedMac) {
        savedMac = generateRandomMac();
        localStorage.setItem('xz_tester_deviceMac', savedMac);
    }
    deviceMacInput.value = savedMac;

    const savedDeviceName = localStorage.getItem('xz_tester_deviceName');
    if (savedDeviceName) {
        deviceNameInput.value = savedDeviceName;
    }

    const savedClientId = localStorage.getItem('xz_tester_clientId');
    if (savedClientId) {
        clientIdInput.value = savedClientId;
    }

    const savedOtaUrl = localStorage.getItem('xz_tester_otaUrl');
    if (savedOtaUrl) {
        otaUrlInput.value = savedOtaUrl;
    }

    otaToken = localStorage.getItem('xz_tester_otaToken') || '';
}

export function saveConfig() {
    const deviceMacInput = document.getElementById('deviceMac');
    const deviceNameInput = document.getElementById('deviceName');
    const clientIdInput = document.getElementById('clientId');

    localStorage.setItem('xz_tester_deviceMac', deviceMacInput.value);
    localStorage.setItem('xz_tester_deviceName', deviceNameInput.value);
    localStorage.setItem('xz_tester_clientId', clientIdInput.value);
}

export function saveOtaResult(result) {
    if (result.websocket && result.websocket.token) {
        otaToken = result.websocket.token;
        localStorage.setItem('xz_tester_otaToken', otaToken);
    }
    if (result.activation && result.activation.code) {
        activationCode = result.activation.code;
    }
}

export function getOtaToken() {
    return otaToken;
}

export function getActivationCode() {
    return activationCode;
}

export function getConfig() {
    const deviceMac = document.getElementById('deviceMac').value.trim();
    return {
        deviceId: deviceMac,
        deviceName: document.getElementById('deviceName').value.trim(),
        deviceMac: deviceMac,
        clientId: document.getElementById('clientId').value.trim(),
        token: otaToken,
    };
}

export function saveConnectionUrls() {
    const otaUrl = document.getElementById('otaUrl').value.trim();
    const wsUrl = document.getElementById('serverUrl').value.trim();
    localStorage.setItem('xz_tester_otaUrl', otaUrl);
    localStorage.setItem('xz_tester_wsUrl', wsUrl);
}
