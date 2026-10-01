import { ESPLoader, Transport } from "./vendor/esptool-js@0.6.0.bundle.js";

const I18N = {
  "zh": {
    headerSub: "通用 ESP 固件 Web 安装器",
    titleFile: "选择固件镜像",
    titleInfo: "固件信息",
    titleFlash: "烧写设备",
    dropHint: "拖入或点击选择固件文件（merged.bin）",
    dropSub: "支持 espflash 产生的整片镜像（合并 bin）或引导程序 bin",
    thChip: "目标芯片", thFlashMode: "Flash 模式", thFlashSize: "Flash 大小",
    thFlashFreq: "Flash 频率", thRevision: "芯片版本", thEntry: "入口地址",
    thSegments: "镜像段数 / 大小", thChecksum: "交叉校验",
    btnConnect: "连接设备", btnFlash: "写入固件",
    browserNote: "要使用浏览器直刷，请使用 Chrome / Edge 桌面版，并通过 USB 连接设备后再发送写入命令。",
    thApp: "应用",
    flashHint: "写入前会先清除整片 Flash；期间不要拔除 USB 线。完成后设备自动重启。",
    connectedState: "设备已连接。请确认下方芯片兼容性，然后点击「写入固件」。",
    dropFile: "请选择 .bin 固件文件。",
    notEspImage: "不是有效的 ESP 固件镜像（开头字节应为 0xE9）。",
    invalidChipId: "镜像头部损坏：无法识别芯片 ID。",
    chipMatch: "芯片匹配，可安全写入",
    chipMismatch: "芯片不匹配！请勿写入该镜像",
    chipUnknown: "无法判断芯片，请谨慎操作",
    parseError: "镜像解析失败",
    connectStart: "正在请求串口……",
    connectFail: "串口请求或连接失败",
    chipDetected: "已连接{chip}",
    writeStart: "开始写入……",
    writeDone: "写入完成，设备已重启。",
    resetHint: "若设备未自动运行新固件，请按板上 RST 按钮或断电后重连。",
    writeFail: "写入失败",
    flashProgress: "写入 {pct}%（{written} / {total} 字节）",
    digestOk: "校验通过",
    digestFail: "校验失败",
  },
  "en": {
    headerSub: "Generic ESP firmware web installer",
    titleFile: "Choose firmware image",
    titleInfo: "Firmware info",
    titleFlash: "Flash device",
    dropHint: "Drag & drop or click to choose firmware (merged.bin)",
    dropSub: "Whole-flash merged images from espflash, or a bootloader bin",
    thChip: "Target chip", thFlashMode: "Flash mode", thFlashSize: "Flash size",
    thFlashFreq: "Flash freq", thRevision: "Chip revision", thEntry: "Entry point",
    thSegments: "Segments / size", thChecksum: "XOR checksum",
    btnConnect: "Connect", btnFlash: "Write firmware",
    browserNote: "Browser flashing requires desktop Chrome / Edge — connect the board over USB before sending the write command.",
    thApp: "App",
    flashHint: "Flash is fully erased before writing. Do not unplug USB during flashing. The device restarts automatically when done.",
    connectedState: "Device connected. Check chip compatibility below, then click “Write firmware”.",
    dropFile: "Please choose a .bin firmware file.",
    notEspImage: "Not a valid ESP firmware image (first byte should be 0xE9).",
    invalidChipId: "Corrupt image header: cannot identify chip ID.",
    chipMatch: "Chip matches, safe to flash",
    chipMismatch: "Chip mismatch! Do NOT flash this image",
    chipUnknown: "Cannot determine chip, proceed with caution",
    parseError: "Failed to parse image",
    connectStart: "Requesting serial port…",
    connectFail: "Serial port request or connect failed",
    chipDetected: "Connected {chip}",
    writeStart: "Flashing…",
    writeDone: "Done. Device restarted.",
    resetHint: "If the device hasn't restarted with the new firmware, press the RST button on the board or power-cycle it.",
    writeFail: "Flash failed",
    flashProgress: "Written {pct}%（{written} / {total} bytes)",
    digestOk: "Checksum OK",
    digestFail: "Checksum failed",
  },
};

const CHIP_NAMES = {
  0: "ESP32", 2: "ESP32-S2", 5: "ESP32-C3", 12: "ESP32-C2",
  13: "ESP32-C6", 16: "ESP32-H2", 18: "ESP32-P4", 20: "ESP32-C61",
  9: "ESP32-S3", 23: "ESP32-C5", 28: "ESP32-H4", 32: "ESP32-S31",
};

function flashSizeName(nibble) {
  const espImageSize = { 0: "1MB", 1: "2MB", 2: "4MB", 3: "8MB", 4: "16MB", 5: "32MB", 6: "64MB", 7: "128MB" };
  const legacySize = { 0: "512KB", 1: "256KB", 2: "1MB", 3: "2MB", 4: "4MB", 5: "8MB", 6: "16MB" };
  return espImageSize[nibble] ?? legacySize[nibble] ?? "?";
}

function flashFreqName(nibble) {
  const map = { 0: "40MHz", 1: "26MHz", 2: "20MHz", 0xf: "80MHz" };
  return map[nibble] ?? "?";
}

function flashModeName(v) {
  return ["QIO", "QOUT", "DIO", "DOUT"][v] ?? "?";
}

function u16le(buf, off) { return buf[off] | (buf[off + 1] << 8); }
function u32le(buf, off) { return (buf[off] | (buf[off + 1] << 8) | (buf[off + 2] << 16) | (buf[off + 3] << 24)) >>> 0; }
function hex(n, w) { return "0x" + n.toString(16).padStart(w, "0"); }
function fmtBytes(n) { return n >= 1048576 ? (n / 1048576).toFixed(2) + " MB" : Math.round(n / 1024) + " KB"; }

let state = { file: null, data: null, info: null, esploader: null };

const $ = (id) => document.getElementById(id);
let lang = "zh";
const t = (k) => I18N[lang][k] ?? k;

function setLang(l) {
  lang = l;
  document.documentElement.lang = l === "zh" ? "zh-CN" : "en";
  document.querySelectorAll("[data-i18n]").forEach((el) => {
    const k = el.dataset.i18n;
    if (I18N[lang][k]) el.textContent = t(k);
  });
  document.getElementById("lang-zh").classList.toggle("active", l === "zh");
  document.getElementById("lang-en").classList.toggle("active", l === "en");
  localStorage.setItem("flasher-lang", l);
}
document.getElementById("lang-zh").onclick = () => setLang("zh");
document.getElementById("lang-en").onclick = () => setLang("en");
setLang(localStorage.getItem("flasher-lang") === "en" ? "en" : "zh");

const dropzone = $("dropzone");
const fileInput = $("fileInput");
dropzone.addEventListener("click", () => fileInput.click());
dropzone.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") fileInput.click(); });
dropzone.addEventListener("dragover", (e) => { e.preventDefault(); dropzone.classList.add("drag"); });
dropzone.addEventListener("dragleave", () => dropzone.classList.remove("drag"));
dropzone.addEventListener("drop", (e) => {
  e.preventDefault(); dropzone.classList.remove("drag");
  if (e.dataTransfer.files.length) void onFile(e.dataTransfer.files[0]);
});
fileInput.addEventListener("change", () => { if (fileInput.files.length) void onFile(fileInput.files[0]); });

function parseImage(data) {
  if (data[0] !== 0xe9) throw new Error(t("notEspImage"));

  const segCount = data[1];
  const flashMode = data[2];
  const flashConfig = data[3];
  const entry = u32le(data, 4);
  const chipId = u16le(data, 12);
  const minRevFull = u16le(data, 15);
  const maxRevFull = u16le(data, 17);
  const appendDigest = data[23];

  let off = 24, totalData = 0, xor = 0xef;
  for (let i = 0; i < segCount; i++) {
    if (off + 8 > data.length) throw new Error(t("parseError"));
    const len = u32le(data, off + 4);
    off += 8;
    if (off + len > data.length) throw new Error(t("parseError"));
    totalData += len;
    for (let j = 0; j < len; j++) xor ^= data[off + j];
    off += len;
  }
  let checksumPos = -1;
  const checksum = xor & 0xff;
  const scanEnd = Math.min(data.length, off + 16);
  for (let i = off; i < scanEnd; i++) {
    if (data[i] === checksum) { checksumPos = i; break; }
  }
  const checksumOk = checksumPos >= 0;
  if (checksumPos < 0) checksumPos = off;
  off = checksumPos + 1;
  while (data[off] === 0xff && off < data.length) off++;
  let padding = (16 - (off % 16)) % 16;
  off += padding;

  let app = null;
  for (let i = 0; i + 4 <= data.length; i++) {
    if (u32le(data, i) === 0xabcd5432) {
      const nullTerm = (buf, s, len) => {
        let e = s; while (e < s + len && buf[e] !== 0) e++; return new TextDecoder().decode(buf.slice(s, e));
      };
      app = {
        version: nullTerm(data, i + 16, 32),
        project: nullTerm(data, i + 48, 32),
        time: nullTerm(data, i + 80, 16),
        date: nullTerm(data, i + 96, 16),
        idf: nullTerm(data, i + 112, 32),
        elfSha: Array.from(data.slice(i + 144, i + 176)).map((b) => b.toString(16).padStart(2, "0")).join(""),
      };
      break;
    }
  }

  return {
    segCount, flashMode, flashSize: flashConfig >> 4, flashFreq: flashConfig & 0xf,
    entry, chipId, minRevFull, maxRevFull, appendDigest, totalData, checksumOk, xor, app, checksumPos,
  };
}

async function onFile(file) {
  try {
    if (!file.name.endsWith(".bin") && !file.name.endsWith(".merged")) throw new Error(t("dropFile"));
    const buf = new Uint8Array(await file.arrayBuffer());
    const info = parseImage(buf);
    state.file = file; state.data = buf; state.info = info;
    $("filemeta").hidden = false;
    const sha = await crypto.subtle.digest("SHA-256", buf);
    $("filemeta").textContent = `${file.name} · ${fmtBytes(buf.length)} · SHA-256 ${Array.from(new Uint8Array(sha)).slice(0, 8).map((b) => b.toString(16).padStart(2, "0")).join("")}…`;
    renderInfo(info, sha);
    $("parsePanel").hidden = false;
    $("flashPanel").hidden = false;
    $("btnConnect").disabled = false;
    $("btnFlash").disabled = true;
    $("chipCheck").textContent = "";
  } catch (e) {
    $("filemeta").hidden = false;
    $("filemeta").textContent = "✗ " + e.message;
    $("chipCheck").innerHTML = "";
    $("btnFlash").disabled = true;
  }
}

function renderInfo(info, sha) {
  const tChip = CHIP_NAMES[info.chipId] ?? `${t("invalidChipId")} (${info.chipId})`;
  $("tChip").textContent = tChip;
  $("tFlashMode").textContent = flashModeName(info.flashMode) + ` (${info.flashMode})`;
  $("tFlashSize").textContent = flashSizeName(info.flashSize) + ` (${info.flashSize})`;
  $("tFlashFreq").textContent = flashFreqName(info.flashFreq) + ` (${info.flashFreq})`;
  $("tRevision").textContent = `v${info.minRevFull / 100}.${info.minRevFull % 100} – v${info.maxRevFull / 100}.${info.maxRevFull % 100}`;
  $("tEntry").textContent = hex(info.entry, 8);
  $("tSegments").textContent = `${info.segCount} 段 / ${fmtBytes(info.totalData)}`;
  $("tChecksum").textContent = `${hex(info.xor & 0xff, 2)} · ` + (info.checksumOk ? t("digestOk") : t("digestFail"));
  $("tChecksum").style.color = info.checksumOk ? "var(--ok)" : "var(--err)";
  $("tSha256").textContent = Array.from(new Uint8Array(sha)).map((b) => b.toString(16).padStart(2, "0")).join("");
  const validity = $("validity");
  if (info.chipId in CHIP_NAMES) {
    validity.textContent = t("digestOk");
    validity.className = "badge " + (info.checksumOk ? "ok" : "warn");
  } else {
    validity.textContent = "⚠";
    validity.className = "badge err";
  }
  const appRow = $("appRow");
  if (info.app && info.app.magic !== undefined) void 0;
  if (info.app) {
    appRow.hidden = false;
    const parts = [info.app.project, info.app.version, info.app.date, info.app.time].filter(Boolean);
    $("tApp").textContent = (parts.join(" · ") || "-") + (info.app.elfSha ? ` · ELF ${info.app.elfSha.slice(0, 16)}` : "");
  } else {
    appRow.hidden = false;
    $("tApp").textContent = "-";
    $("tApp").style.color = "var(--muted)";
  }
}

function log(msg, cls = "info") {
  const el = document.createElement("div");
  el.className = cls;
  el.textContent = msg;
  $("log").appendChild(el);
  $("log").scrollTop = $("log").scrollHeight;
}

function normalizeChip(name) {
  return (name || "").toLowerCase().replace(/\([^)]*\)/g, "").replace(/[^a-z0-9]/g, "");
}

$("btnConnect").addEventListener("click", async () => {
  $("btnConnect").disabled = true;
  $("btnFlash").disabled = true;
  $("chipCheck").innerHTML = "";
  try {
    $("state").hidden = false;
    log(t("connectStart"));
    const port = await navigator.serial.requestPort();
    if (state.esploader?.transport) {
      try { await state.esploader.transport.disconnect(); } catch (_) { /* ignore */ }
    }
    state.esploader = null;
    const transport = new Transport(port, true);
    const esploader = new ESPLoader({
      transport,
      baudrate: 115200,
      terminal: { clean: () => {}, writeLine: (m) => log("· " + m), write: (m) => log("· " + m) },
    });
    const chipNameRaw = await esploader.main("default_reset");
    state.esploader = esploader;
    const chipName = normalizeChip(chipNameRaw);
    log(t("chipDetected").replace("{chip}", chipNameRaw), "ok");

    const binChip = normalizeChip(CHIP_NAMES[state.info.chipId] ?? "");
    const check = $("chipCheck");
    const badge = document.createElement("span");
    if (binChip && chipName) {
      const matched = binChip === chipName;
      badge.className = "badge " + (matched ? "ok" : "err");
      badge.textContent = matched ? t("chipMatch") : t("chipMismatch");
      $("btnFlash").disabled = !matched;
    } else {
      badge.className = "badge warn";
      badge.textContent = t("chipUnknown");
    }
    check.appendChild(badge);
  } catch (e) {
    log(t("connectFail") + ": " + e.message, "err");
  } finally {
    $("btnConnect").disabled = false;
  }
});

$("btnFlash").addEventListener("click", async () => {
  const esploader = state.esploader;
  if (!esploader || !state.data) return;
  try {
    $("btnFlash").disabled = true;
    log(t("writeStart"), "info");
    $("progressbar").style.width = "0%";
    await esploader.writeFlash({
      fileArray: [{ data: state.data, address: 0x0 }],
      flashMode: "keep",
      flashFreq: "keep",
      flashSize: "keep",
      eraseAll: true,
      compress: true,
      reportProgress: (fileIndex, written, total) => {
        const pct = Math.min(100, Math.round((written / total) * 100));
        $("progressbar").style.width = pct + "%";
        $("progresstext").textContent = t("flashProgress").replace("{pct}", pct).replace("{written}", written).replace("{total}", total);
      },
    });
    $("progressbar").style.width = "100%";
    $("progresstext").textContent = "";
    log(t("writeDone"), "ok");
    try { await esploader.after("hard_reset"); } catch (_) { /* ignore reset failure */ }
    log(t("resetHint"), "info");
  } catch (e) {
    log(t("writeFail") + ": " + e.message, "err");
  } finally {
    if (state.esploader?.transport) {
      try { await state.esploader.transport.disconnect(); } catch (_) { /* ignore */ }
    }
    state.esploader = null;
    $("chipCheck").innerHTML = "";
    $("btnConnect").disabled = false;
  }
});