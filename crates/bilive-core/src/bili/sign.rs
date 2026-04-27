use std::time::{SystemTime, UNIX_EPOCH};
use url::form_urlencoded::Serializer;

const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

const APP_KEY: &str = "aae92bc66f3edfab";
const APP_SEC: &str = "af125a0d5279fd576c1b4418a3e8276d";

pub fn wbi_sign(params: Vec<(&str, String)>, img_key: &str, sub_key: &str) -> String {
    let mut entries: Vec<(String, String)> = params
        .into_iter()
        .map(|(key, value)| (key.to_string(), clean_wbi_value(&value)))
        .collect();
    entries.push(("wts".to_string(), unix_seconds().to_string()));
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let query = encode_pairs(
        entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    );
    let rid = format!(
        "{:x}",
        md5::compute(format!("{}{}", query, mixin_key(img_key, sub_key)))
    );
    format!("{query}&w_rid={rid}")
}

pub fn app_sign(mut params: Vec<(&str, String)>) -> String {
    params.push(("appkey", APP_KEY.to_string()));
    params.sort_by(|left, right| left.0.cmp(right.0));

    let query = encode_pairs(params.iter().map(|(key, value)| (*key, value.as_str())));
    let sign = format!("{:x}", md5::compute(format!("{query}{APP_SEC}")));
    format!("{query}&sign={sign}")
}

fn mixin_key(img_key: &str, sub_key: &str) -> String {
    let source = format!("{}{}", extract_key(img_key), extract_key(sub_key));
    let chars = source.chars().collect::<Vec<_>>();
    MIXIN_KEY_ENC_TAB
        .iter()
        .filter_map(|index| chars.get(*index))
        .take(32)
        .collect()
}

fn extract_key(value: &str) -> String {
    let filename = value.rsplit('/').next().unwrap_or(value);
    filename.split('.').next().unwrap_or(filename).to_string()
}

fn clean_wbi_value(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, '!' | '\'' | '(' | ')' | '*'))
        .collect()
}

fn encode_pairs<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut serializer = Serializer::new(String::new());
    for (key, value) in pairs {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_sign_orders_params() {
        let signed = app_sign(vec![
            ("ts", "1".to_string()),
            ("system_version", "2".to_string()),
        ]);
        assert!(signed.starts_with("appkey=aae92bc66f3edfab&system_version=2&ts=1&sign="));
    }

    #[test]
    fn wbi_sign_adds_required_fields() {
        let signed = wbi_sign(
            vec![("id", "1".to_string()), ("type", "0".to_string())],
            "7cd084941338484aae1ad9425b84077c",
            "4932caff0ff746eab6f01bf08b70ac45",
        );
        assert!(signed.contains("id=1"));
        assert!(signed.contains("type=0"));
        assert!(signed.contains("wts="));
        assert!(signed.contains("w_rid="));
    }
}
