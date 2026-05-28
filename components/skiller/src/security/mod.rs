pub fn contains_secret_like_content(text: &str) -> bool {
    let t = text.to_lowercase();
    ["api_key=", "apikey=", "password=", "token=", "secret="]
        .iter()
        .any(|p| t.contains(p))
}
