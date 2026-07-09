#![allow(dead_code)]

use std::fmt;

/// ASR transcription diagnostics.
#[derive(Debug)]
pub struct AsrDiagnostics {
    pub audio_duration_secs: f64,
    pub asr_elapsed_secs: f64,
    pub rtf: f64,
    pub cer: f64,
    pub wer: f64,
}

impl AsrDiagnostics {
    pub fn accuracy(&self) -> f64 {
        1.0 - self.cer
    }

    pub fn accuracy_grade(&self) -> &'static str {
        match self.cer {
            c if c < 0.03 => "A",
            c if c < 0.06 => "B",
            c if c < 0.10 => "C",
            c if c < 0.20 => "D",
            _ => "F",
        }
    }

    pub fn performance_grade(&self) -> &'static str {
        match self.rtf {
            r if r < 0.05 => "A",
            r if r < 0.10 => "B",
            r if r < 0.20 => "C",
            r if r < 0.50 => "D",
            _ => "F",
        }
    }

    pub fn score(&self) -> f64 {
        let acc = (1.0 - self.cer).clamp(0.0, 1.0) * 70.0;
        let perf = (1.0 - self.rtf.min(1.0)) * 30.0;
        acc + perf
    }

    pub fn verdict(&self) -> &'static str {
        match self.cer {
            c if c >= 0.10 => "Unsuitable - CER exceeds usability threshold",
            c if c >= 0.05 => "Marginal - noticeable recognition errors",
            _ => match self.rtf {
                r if r >= 0.50 => "Marginal - real-time factor too high",
                _ => "Suitable for daily use - all indicators within normal range",
            },
        }
    }
}

impl fmt::Display for AsrDiagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CER={:.2}%({}) WER={:.2}% Acc={:.1}% Perf={:.1}% Total={:.1}% | \
             RTF={:.3} ASR={:.1}s Audio={:.2}s {}",
            self.cer * 100.0,
            self.accuracy_grade(),
            self.wer * 100.0,
            self.accuracy() * 100.0,
            (1.0 - self.rtf.min(1.0)) * 100.0,
            self.score(),
            self.rtf,
            self.asr_elapsed_secs,
            self.audio_duration_secs,
            self.verdict(),
        )
    }
}

/// Analyze ASR transcription result.
pub fn analyze_asr(
    audio_duration_secs: f64,
    asr_elapsed: std::time::Duration,
    reference: &str,
    hypothesis: &str,
) -> AsrDiagnostics {
    AsrDiagnostics {
        audio_duration_secs,
        asr_elapsed_secs: asr_elapsed.as_secs_f64(),
        rtf: asr_elapsed.as_secs_f64() / audio_duration_secs.max(0.001),
        cer: cer(reference, hypothesis),
        wer: wer(reference, hypothesis),
    }
}

/// Remove punctuation (CJK + ASCII) and lowercase ASCII chars.
fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !c.is_ascii_punctuation()
                && !matches!(
                    c,
                    '。' | '，'
                        | '、'
                        | '？'
                        | '！'
                        | '；'
                        | '：'
                        | '“'
                        | '”'
                        | '‘'
                        | '’'
                        | '（'
                        | '）'
                        | '【'
                        | '】'
                        | '《'
                        | '》'
                )
        })
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Character Error Rate: Levenshtein distance at character level.
fn cer(reference: &str, hypothesis: &str) -> f64 {
    let ref_chars: Vec<char> = normalize(reference).chars().collect();
    let hyp_chars: Vec<char> = normalize(hypothesis).chars().collect();
    let n = ref_chars.len();
    if n == 0 {
        return 0.0;
    }
    let mut prev: Vec<usize> = (0..=hyp_chars.len()).collect();
    for (i, rc) in ref_chars.iter().enumerate() {
        let mut cur = vec![i + 1; hyp_chars.len() + 1];
        for (j, hc) in hyp_chars.iter().enumerate() {
            let cost = if rc == hc { 0 } else { 1 };
            cur[j + 1] = std::cmp::min(cur[j] + 1, std::cmp::min(prev[j + 1] + 1, prev[j] + cost));
        }
        prev = cur;
    }
    *prev.last().unwrap() as f64 / n as f64
}

/// Word Error Rate: Levenshtein distance at whitespace-delimited word level.
fn wer(reference: &str, hypothesis: &str) -> f64 {
    let ref_norm = normalize(reference);
    let hyp_norm = normalize(hypothesis);
    let ref_words: Vec<&str> = ref_norm.split_whitespace().collect();
    let hyp_words: Vec<&str> = hyp_norm.split_whitespace().collect();
    let n = ref_words.len();
    if n == 0 {
        return 0.0;
    }
    let mut prev: Vec<usize> = (0..=hyp_words.len()).collect();
    for (i, rw) in ref_words.iter().enumerate() {
        let mut cur = vec![i + 1; hyp_words.len() + 1];
        for (j, hw) in hyp_words.iter().enumerate() {
            let cost = if rw == hw { 0 } else { 1 };
            cur[j + 1] = std::cmp::min(cur[j] + 1, std::cmp::min(prev[j + 1] + 1, prev[j] + cost));
        }
        prev = cur;
    }
    *prev.last().unwrap() as f64 / n as f64
}
