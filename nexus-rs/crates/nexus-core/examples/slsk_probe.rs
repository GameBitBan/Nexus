fn assert_send<T: Send>() {}
fn assert_sync<T: Sync>() {}
fn main() {
    assert_send::<soulseek_rs::Client>();
    assert_sync::<soulseek_rs::Client>();
    println!("ok");
}
