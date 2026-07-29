import { log } from '../utils/logger.js';
import { loadConfig, saveConfig } from '../config/manager.js';
import { getAudioPlayer } from '../core/audio/player.js';
import { getAudioRecorder } from '../core/audio/recorder.js';
import { getWebSocketHandler } from '../core/network/websocket.js';
import { registerDevice } from '../core/network/ota-connector.js';

export class UIController {
    constructor() {
        this.isEditing = false;
        this.visualizerCanvas = null;
        this.visualizerContext = null;
        this.audioStatsTimer = null;
        this.otaResult = null;
    }

    init() {
        this.visualizerCanvas = document.getElementById('audioVisualizer');
        this.visualizerContext = this.visualizerCanvas.getContext('2d');

        this.initVisualizer();
        this.initEventListeners();
        this.startAudioStatsMonitor();
        loadConfig();
    }

    initVisualizer() {
        this.visualizerCanvas.width = this.visualizerCanvas.clientWidth;
        this.visualizerCanvas.height = this.visualizerCanvas.clientHeight;
        this.visualizerContext.fillStyle = '#fafafa';
        this.visualizerContext.fillRect(0, 0, this.visualizerCanvas.width, this.visualizerCanvas.height);
    }

    updateStatusDisplay(element, text) {
        element.textContent = text;
        element.removeAttribute('style');
        element.classList.remove('connected');
        if (text.includes('已连接')) {
            element.classList.add('connected');
        }
    }

    updateConnectionUI(isConnected) {
        const connectionStatus = document.getElementById('connectionStatus');
        const connectButton = document.getElementById('connectButton');
        const messageInput = document.getElementById('messageInput');
        const sendTextButton = document.getElementById('sendTextButton');
        const recordButton = document.getElementById('recordButton');

        if (isConnected) {
            this.updateStatusDisplay(connectionStatus, '● WS已连接');
            connectButton.textContent = '断开';
            messageInput.disabled = false;
            sendTextButton.disabled = false;
            recordButton.disabled = false;
        } else {
            this.updateStatusDisplay(connectionStatus, '● WS未连接');
            connectButton.textContent = '连接';
            messageInput.disabled = true;
            sendTextButton.disabled = true;
            recordButton.disabled = true;
            this.updateSessionStatus(null);
        }
    }

    updateRecordButtonState(isRecording, seconds = 0) {
        const recordButton = document.getElementById('recordButton');
        if (isRecording) {
            recordButton.textContent = `停止录音 ${seconds.toFixed(1)}秒`;
            recordButton.classList.add('recording');
        } else {
            recordButton.textContent = '开始录音';
            recordButton.classList.remove('recording');
        }
        recordButton.disabled = false;
    }

    updateSessionStatus(isSpeaking) {
        const sessionStatus = document.getElementById('sessionStatus');
        if (!sessionStatus) return;

        const bgHtml = '<span id="sessionStatusBg" style="position: absolute; left: 0; top: 0; bottom: 0; width: 0%; background: linear-gradient(90deg, rgba(76, 175, 80, 0.2), rgba(33, 150, 243, 0.2)); transition: width 0.15s ease-out, background 0.3s ease; z-index: 0; border-radius: 20px;"></span>';

        if (isSpeaking === null) {
            sessionStatus.innerHTML = bgHtml + '<span style="position: relative; z-index: 1;"><span class="emoji-large">😶</span> 小智离线中</span>';
            sessionStatus.className = 'status offline';
        } else if (isSpeaking) {
            sessionStatus.innerHTML = bgHtml + '<span style="position: relative; z-index: 1;"><span class="emoji-large">😶</span> 小智说话中</span>';
            sessionStatus.className = 'status speaking';
        } else {
            sessionStatus.innerHTML = bgHtml + '<span style="position: relative; z-index: 1;"><span class="emoji-large">😶</span> 小智聆听中</span>';
            sessionStatus.className = 'status listening';
        }
    }

    updateSessionEmotion(emoji) {
        const sessionStatus = document.getElementById('sessionStatus');
        if (!sessionStatus) return;

        let currentText = sessionStatus.textContent;
        currentText = currentText.replace(/[\u{1F300}-\u{1F9FF}]|[\u{2600}-\u{26FF}]|[\u{2700}-\u{27BF}]/gu, '').trim();

        const bgHtml = '<span id="sessionStatusBg" style="position: absolute; left: 0; top: 0; bottom: 0; width: 0%; background: linear-gradient(90deg, rgba(76, 175, 80, 0.2), rgba(33, 150, 243, 0.2)); transition: width 0.15s ease-out, background 0.3s ease; z-index: 0; border-radius: 20px;"></span>';

        sessionStatus.innerHTML = bgHtml + `<span style="position: relative; z-index: 1;"><span class="emoji-large">${emoji}</span> ${currentText}</span>`;
    }

    showActivationCode(code) {
        const container = document.getElementById('activationContainer');
        if (!container) return;
        container.style.display = 'block';
        document.getElementById('activationCodeDisplay').textContent = code;
    }

    hideActivationCode() {
        const container = document.getElementById('activationContainer');
        if (container) {
            container.style.display = 'none';
        }
    }

    updateAudioStats() {
        const audioPlayer = getAudioPlayer();
        const stats = audioPlayer.getAudioStats();

        const sessionStatus = document.getElementById('sessionStatus');
        const sessionStatusBg = document.getElementById('sessionStatusBg');

        if (sessionStatus && sessionStatus.classList.contains('speaking') && sessionStatusBg) {
            if (stats.pendingPlay > 0) {
                let percentage;
                if (stats.pendingPlay >= 10) {
                    percentage = 100;
                } else {
                    percentage = (stats.pendingPlay / 10) * 100;
                }

                sessionStatusBg.style.width = `${percentage}%`;

                if (stats.pendingPlay < 5) {
                    sessionStatusBg.style.background = 'linear-gradient(90deg, rgba(255, 152, 0, 0.25), rgba(255, 87, 34, 0.25))';
                } else if (stats.pendingPlay < 10) {
                    sessionStatusBg.style.background = 'linear-gradient(90deg, rgba(205, 220, 57, 0.25), rgba(76, 175, 80, 0.25))';
                } else {
                    sessionStatusBg.style.background = 'linear-gradient(90deg, rgba(76, 175, 80, 0.25), rgba(33, 150, 243, 0.25))';
                }
            } else {
                sessionStatusBg.style.width = '0%';
            }
        } else {
            if (sessionStatusBg) {
                sessionStatusBg.style.width = '0%';
            }
        }
    }

    startAudioStatsMonitor() {
        this.audioStatsTimer = setInterval(() => {
            this.updateAudioStats();
        }, 100);
    }

    stopAudioStatsMonitor() {
        if (this.audioStatsTimer) {
            clearInterval(this.audioStatsTimer);
            this.audioStatsTimer = null;
        }
    }

    drawVisualizer(dataArray) {
        this.visualizerContext.fillStyle = '#fafafa';
        this.visualizerContext.fillRect(0, 0, this.visualizerCanvas.width, this.visualizerCanvas.height);

        const barWidth = (this.visualizerCanvas.width / dataArray.length) * 2.5;
        let barHeight;
        let x = 0;

        for (let i = 0; i < dataArray.length; i++) {
            barHeight = dataArray[i] / 2;

            const hue = 200 + (barHeight / this.visualizerCanvas.height) * 60;
            const saturation = 80 + (barHeight / this.visualizerCanvas.height) * 20;
            const lightness = 45 + (barHeight / this.visualizerCanvas.height) * 15;

            this.visualizerContext.fillStyle = `hsl(${hue}, ${saturation}%, ${lightness}%)`;
            this.visualizerContext.fillRect(x, this.visualizerCanvas.height - barHeight, barWidth, barHeight);

            x += barWidth + 1;
        }
    }

    initEventListeners() {
        const wsHandler = getWebSocketHandler();
        const audioRecorder = getAudioRecorder();

        wsHandler.onConnectionStateChange = (isConnected) => {
            this.updateConnectionUI(isConnected);
        };

        wsHandler.onRecordButtonStateChange = (isRecording) => {
            this.updateRecordButtonState(isRecording);
        };

        wsHandler.onSessionStateChange = (isSpeaking) => {
            this.updateSessionStatus(isSpeaking);
        };

        wsHandler.onSessionEmotionChange = (emoji) => {
            this.updateSessionEmotion(emoji);
        };

        audioRecorder.onRecordingStart = (seconds) => {
            this.updateRecordButtonState(true, seconds);
        };

        audioRecorder.onRecordingStop = () => {
            this.updateRecordButtonState(false);
        };

        audioRecorder.onVisualizerUpdate = (dataArray) => {
            this.drawVisualizer(dataArray);
        };

        const connectButton = document.getElementById('connectButton');
        let isConnecting = false;

        const handleConnect = async () => {
            if (isConnecting) return;

            if (wsHandler.isConnected()) {
                wsHandler.disconnect();
                return;
            }

            isConnecting = true;
            try {
                const otaUrl = document.getElementById('otaUrl').value.trim();
                const otaResult = await registerDevice(otaUrl);
                if (!otaResult) {
                    log('OTA注册失败', 'error');
                    isConnecting = false;
                    return;
                }

                this.otaResult = otaResult;

                const activation = otaResult.activation;
                if (activation && activation.code) {
                    this.showActivationCode(activation.code);
                    log(`设备未激活，激活码: ${activation.code}`, 'info');
                    isConnecting = false;
                    return;
                }

                this.hideActivationCode();

                const wsOk = await wsHandler.connect(otaResult);
                if (!wsOk) {
                    log('WebSocket连接失败', 'error');
                }
            } catch (error) {
                log(`连接过程出错: ${error.message}`, 'error');
            }
            isConnecting = false;
        };

        connectButton.addEventListener('click', handleConnect);

        const toggleButton = document.getElementById('toggleConfig');
        const deviceMacInput = document.getElementById('deviceMac');
        const deviceNameInput = document.getElementById('deviceName');
        const clientIdInput = document.getElementById('clientId');

        toggleButton.addEventListener('click', () => {
            this.isEditing = !this.isEditing;

            deviceMacInput.disabled = !this.isEditing;
            deviceNameInput.disabled = !this.isEditing;
            clientIdInput.disabled = !this.isEditing;

            toggleButton.textContent = this.isEditing ? '确定' : '编辑';

            if (!this.isEditing) {
                saveConfig();
            }
        });

        const tabs = document.querySelectorAll('.tab');
        tabs.forEach(tab => {
            tab.addEventListener('click', () => {
                tabs.forEach(t => t.classList.remove('active'));
                document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));

                tab.classList.add('active');
                const tabContent = document.getElementById(`${tab.dataset.tab}Tab`);
                tabContent.classList.add('active');

                if (tab.dataset.tab === 'voice') {
                    setTimeout(() => {
                        this.initVisualizer();
                    }, 50);
                }
            });
        });

        const messageInput = document.getElementById('messageInput');
        const sendTextButton = document.getElementById('sendTextButton');

        const sendMessage = () => {
            const message = messageInput.value.trim();
            if (message && wsHandler.sendTextMessage(message)) {
                messageInput.value = '';
            }
        };

        sendTextButton.addEventListener('click', sendMessage);
        messageInput.addEventListener('keypress', (e) => {
            if (e.key === 'Enter') sendMessage();
        });

        const recordButton = document.getElementById('recordButton');
        recordButton.addEventListener('click', () => {
            if (audioRecorder.isRecording) {
                audioRecorder.stop();
            } else {
                audioRecorder.start();
            }
        });

        window.addEventListener('resize', () => this.initVisualizer());
    }
}

let uiControllerInstance = null;

export function getUIController() {
    if (!uiControllerInstance) {
        uiControllerInstance = new UIController();
    }
    return uiControllerInstance;
}
