#[derive(PartialEq, Debug, Clone)]
pub struct GameStatus {
    pub state: String,
    pub installdir: String,
    pub size: f64,
}

impl GameStatus {
    pub fn msg(maybe_status: &Option<GameStatus>, data: &str) -> GameStatus {
        match maybe_status {
            Some(status) => GameStatus {
                state: data.to_string(),
                installdir: status.installdir.clone(),
                size: status.size,
            },
            None => GameStatus {
                state: data.to_string(),
                installdir: "".to_string(),
                size: 0.,
            },
        }
    }
}
