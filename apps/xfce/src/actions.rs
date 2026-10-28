//! Assignable Xfce window actions (not tiling or fullscreen toggles).
#[cfg(any(feature = "gui", test))]
#[path = "actions/geometry.rs"]
pub mod geometry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Fill,
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    Center,
    NextOutput,
    PreviousOutput,
    Close,
}
impl Action {
    pub const ALL: [Self; 9] = [
        Self::Fill,
        Self::LeftHalf,
        Self::RightHalf,
        Self::TopHalf,
        Self::BottomHalf,
        Self::Center,
        Self::NextOutput,
        Self::PreviousOutput,
        Self::Close,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::LeftHalf => "left-half",
            Self::RightHalf => "right-half",
            Self::TopHalf => "top-half",
            Self::BottomHalf => "bottom-half",
            Self::Center => "center",
            Self::NextOutput => "next-output",
            Self::PreviousOutput => "previous-output",
            Self::Close => "close",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Fill => "Fill Screen",
            Self::LeftHalf => "Move to Left Half",
            Self::RightHalf => "Move to Right Half",
            Self::TopHalf => "Move to Top Half",
            Self::BottomHalf => "Move to Bottom Half",
            Self::Center => "Center Window",
            Self::NextOutput => "Move to Next Output",
            Self::PreviousOutput => "Move to Previous Output",
            Self::Close => "Close Window",
        }
    }
    pub fn parse(value: &str) -> crate::x11::Result<Self> {
        Self::ALL
            .into_iter()
            .find(|action| action.id() == value)
            .ok_or_else(|| "unknown action; use actions to list supported IDs".into())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn action_ids_are_closed_and_round_trip() {
        for action in Action::ALL {
            assert_eq!(Action::parse(action.id()).unwrap(), action);
            assert_eq!(
                serde_json::from_str::<Action>(&serde_json::to_string(&action).unwrap()).unwrap(),
                action
            );
        }
        assert!(Action::parse("fill;kill").is_err());
        assert!(Action::parse("tile").is_err());
    }
}
