use std::collections::HashMap;
use std::env::var;
use std::str::FromStr;

use serde::Deserialize;

macro_rules! config_env {
    ($struct:ident { $($cfg:ident: $ty:ty = $default:expr => $default_fn_str:expr,)* }) => {
        #[derive(Debug, Clone, Deserialize)]
        pub struct $struct {
        $(
            #[serde(default = $default_fn_str)]
            pub $cfg: $ty
        ),*
        }

        $(
        fn $cfg() -> $ty {
            var(
                &format!("FRIDAY_{}_{}", stringify!($struct).to_uppercase(), stringify!($cfg).to_uppercase())
            ).map(|v| <$ty>::from_str(&v).unwrap()).unwrap_or($default.into())
        }
        )*

        impl Default for $struct {
            fn default() -> Self {
                $struct {
                    $(
                        $cfg: var(
                            &format!("FRIDAY_{}_{}", stringify!($struct).to_uppercase(), stringify!($cfg).to_uppercase())
                        ).map(|v| <$ty>::from_str(&v).unwrap()).unwrap_or($default.into())
                    ),*
                }
            }
        }
    };
}

// TODO use https://github.com/rust-lang/rfcs/pull/3681
config_env! {
    Server {
        bind_address: String = "0.0.0.0" => "bind_address",
        port: u16 = 3000u16 => "port",
        database_url: String = "sqlite::memory:" => "database_url",
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    pub server: Server,
}
