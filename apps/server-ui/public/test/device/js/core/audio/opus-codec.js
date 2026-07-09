import { log } from '../../utils/logger.js';
import { updateScriptStatus } from '../../ui/dom-helper.js'


// 检查Opus库是否已加载
export function checkOpusLoaded() {
    try {
        // 检查Module是否存在（本地库导出的全局变量）
        if (typeof Module === 'undefined') {
            throw new Error('Opus库未加载，Module对象不存在');
        }

        // 尝试先使用Module.instance（libopus.js最后一行导出方式）
        if (typeof Module.instance !== 'undefined' && typeof Module.instance._opus_decoder_get_size === 'function') {
            // 使用Module.instance对象替换全局Module对象
            window.ModuleInstance = Module.instance;
            log('Opus库加载成功（使用Module.instance）', 'success');
            updateScriptStatus('Opus库加载成功', 'success');

            // 3秒后隐藏状态
            const statusElement = document.getElementById('scriptStatus');
            if (statusElement) statusElement.style.display = 'none';
            return;
        }

        // 如果没有Module.instance，检查全局Module函数
        if (typeof Module._opus_decoder_get_size === 'function') {
            window.ModuleInstance = Module;
            log('Opus库加载成功（使用全局Module）', 'success');
            updateScriptStatus('Opus库加载成功', 'success');

            // 3秒后隐藏状态
            const statusElement = document.getElementById('scriptStatus');
            if (statusElement) statusElement.style.display = 'none';
            return;
        }

        throw new Error('Opus解码函数未找到，可能Module结构不正确');
    } catch (err) {
        log(`Opus库加载失败，请检查libopus.js文件是否存在且正确: ${err.message}`, 'error');
        updateScriptStatus('Opus库加载失败，请检查libopus.js文件是否存在且正确', 'error');
    }
}


// 创建一个Opus编码器
let opusEncoder = null;
export function initOpusEncoder(sampleRate = 16000) {
    if (opusEncoder) {
        return opusEncoder;
    }

    if (!window.ModuleInstance) {
        log('无法创建Opus编码器：ModuleInstance不可用', 'error');
        return;
    }

    const mod = window.ModuleInstance;
    const channels = 1;
    const application = 2048; // OPUS_APPLICATION_VOIP
    const frameSize = Math.round(sampleRate * 20 / 1000); // 20ms帧

    opusEncoder = {
        channels,
        sampleRate,
        frameSize,
        maxPacketSize: 4000,
        module: mod,
        bitrate: 24000, // 24kbps (之前16kbps，提高品质)
        encoderPtr: null,

        init() {
            try {
                const encoderSize = mod._opus_encoder_get_size(this.channels);
                this.encoderPtr = mod._malloc(encoderSize);
                if (!this.encoderPtr) {
                    throw new Error("无法分配编码器内存");
                }

                const err = mod._opus_encoder_init(
                    this.encoderPtr,
                    this.sampleRate,
                    this.channels,
                    application
                );

                if (err < 0) {
                    throw new Error(`Opus编码器初始化失败: ${err}`);
                }

                // 设置位率 (24kbps)
                mod._opus_encoder_ctl(this.encoderPtr, 4002, this.bitrate);

                // 设置复杂度
                mod._opus_encoder_ctl(this.encoderPtr, 4010, 5);

                // 禁用DTX — 避免静音→语音过渡时产生方波伪影
                mod._opus_encoder_ctl(this.encoderPtr, 4016, 0);

                return true;
            } catch (error) {
                if (this.encoderPtr) {
                    mod._free(this.encoderPtr);
                    this.encoderPtr = null;
                }
                log(`Opus编码器初始化失败: ${error.message}`, 'error');
                return false;
            }
        },

        encode(pcmData) {
            if (!this.encoderPtr && !this.init()) {
                return null;
            }

            try {
                const mod = this.module;
                const pcmPtr = mod._malloc(pcmData.length * 2);
                for (let i = 0; i < pcmData.length; i++) {
                    mod.HEAP16[(pcmPtr >> 1) + i] = pcmData[i];
                }

                const outPtr = mod._malloc(this.maxPacketSize);
                const encodedLen = mod._opus_encode(
                    this.encoderPtr, pcmPtr, this.frameSize, outPtr, this.maxPacketSize,
                );

                mod._free(pcmPtr);

                if (encodedLen < 0) {
                    mod._free(outPtr);
                    throw new Error(`Opus编码失败: ${encodedLen}`);
                }

                const opusData = new Uint8Array(encodedLen);
                for (let i = 0; i < encodedLen; i++) {
                    opusData[i] = mod.HEAPU8[outPtr + i];
                }
                mod._free(outPtr);

                return opusData;
            } catch (error) {
                log(`Opus编码出错: ${error.message}`, 'error');
                return null;
            }
        },

        destroy() {
            if (this.encoderPtr) {
                this.module._free(this.encoderPtr);
                this.encoderPtr = null;
            }
        },
    };

    opusEncoder.init();
    return opusEncoder;
}