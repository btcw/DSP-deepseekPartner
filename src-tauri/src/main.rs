fn main() {
    if std::env::var("DEEPSEEK_GATEWAY_HEADLESS").as_deref() == Ok("1") {
        deepseek_gateway_desktop_lib::run_headless();
    } else {
        deepseek_gateway_desktop_lib::run()
    }
}
