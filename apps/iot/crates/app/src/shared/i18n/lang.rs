//! The languages the product prints in.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    En,
    ZhHans,
}

impl Lang {
    pub const ALL: &[Self] = &[Self::En, Self::ZhHans];

    /// The name a build states in `IOT_LANG`; unknown names fall back to [`Self::default`].
    pub const fn from_name(name: &str) -> Option<Self> {
        let name = name.trim_ascii().as_bytes();
        if eq(name, b"zh") || eq(name, b"zh-hans") || eq(name, b"zh_cn") {
            Some(Self::ZhHans)
        } else if eq(name, b"en") || eq(name, b"en-us") || eq(name, b"en_gb") {
            Some(Self::En)
        } else {
            None
        }
    }
}

const fn eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if !left[index].eq_ignore_ascii_case(&right[index]) {
            return false;
        }
        index += 1;
    }
    true
}

/// One piece of product copy. A new variant must answer both languages.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_language_name_survives_case_and_space() {
        assert_eq!(Lang::from_name(" ZH "), Some(Lang::ZhHans));
        assert_eq!(Lang::from_name("zh-Hans"), Some(Lang::ZhHans));
        assert_eq!(Lang::from_name("EN"), Some(Lang::En));
        assert_eq!(Lang::from_name("fr"), None);
        assert_eq!(Lang::from_name(""), None);
    }
}
