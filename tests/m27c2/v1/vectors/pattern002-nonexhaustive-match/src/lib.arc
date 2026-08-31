pub enum Coin {
    Heads,
    Tails,
}
pub fn classify(coin: Coin) -> i32 {
    match coin {
        Coin::Heads => 1i32,
    }
}
