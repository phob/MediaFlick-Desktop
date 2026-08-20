use serde_json::{Value, json};

use super::auth::same_media_server_user;
use super::{SeerrSession, SeerrState};
use crate::library::{Library, SeerrConfig};
use crate::seerr::api::client::{SeerrClient, SessionCookies};
use crate::seerr::api::error::SeerrError;
use crate::seerr::discovery::UtcDate;
use crate::seerr::{DiscoverKind, DiscoverOptions, RequestProfileSelection};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

mod auth;
mod catalog;
mod lifecycle;
mod requests;
mod support;
