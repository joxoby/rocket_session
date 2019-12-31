use json_dotpath::DotPaths;
use parking_lot::RwLock;
use rand::Rng;
use rocket::fairing::{self, Fairing, Info};
use rocket::request::FromRequest;

use rocket::{
    http::{Cookie, Status},
    Outcome, Request, Response, Rocket, State,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};

use std::collections::HashMap;
use std::ops::Add;
use std::time::{Duration, Instant};

const SESSION_ID: &str = "SESSID";

type SessionsMap = HashMap<String, SessionInstance>;

#[derive(Debug)]
struct SessionInstance {
    data: serde_json::Map<String, Value>,
    expires: Instant,
}

#[derive(Default, Debug)]
struct SessionStore {
    inner: RwLock<SessionsMap>,
    lifespan: Duration,
}

#[derive(PartialEq, Hash, Clone, Debug)]
struct SessionID(String);

impl<'a, 'r> FromRequest<'a, 'r> for &'a SessionID {
    type Error = ();

    fn from_request(request: &'a Request<'r>) -> Outcome<Self, (Status, Self::Error), ()> {
        Outcome::Success(request.local_cache(|| {
            println!("get id");
            if let Some(cookie) = request.cookies().get(SESSION_ID) {
                println!("from cookie");
                SessionID(cookie.value().to_string()) // FIXME avoid cloning (cow?)
            } else {
                println!("new id");
                SessionID(
                    rand::thread_rng()
                        .sample_iter(&rand::distributions::Alphanumeric)
                        .take(16)
                        .collect(),
                )
            }
        }))
    }
}

#[derive(Debug)]
pub struct Session<'a> {
    store: State<'a, SessionStore>,
    id: &'a SessionID,
}

impl<'a, 'r> FromRequest<'a, 'r> for Session<'a> {
    type Error = ();

    fn from_request(request: &'a Request<'r>) -> Outcome<Self, (Status, Self::Error), ()> {
        Outcome::Success(Session {
            id: request.local_cache(|| {
                if let Some(cookie) = request.cookies().get(SESSION_ID) {
                    SessionID(cookie.value().to_string())
                } else {
                    SessionID(
                        rand::thread_rng()
                            .sample_iter(&rand::distributions::Alphanumeric)
                            .take(16)
                            .collect(),
                    )
                }
            }),
            store: request.guard().unwrap(),
        })
    }
}

impl<'a> Session<'a> {
    pub fn fairing(lifespan: Duration) -> impl Fairing {
        SessionFairing { lifespan }
    }

    pub fn tap<T>(&self, func: impl FnOnce(&mut serde_json::Map<String, Value>) -> T) -> T {
        let mut wg = self.store.inner.write();
        if let Some(instance) = wg.get_mut(&self.id.0) {
            instance.expires = Instant::now().add(self.store.lifespan);
            func(&mut instance.data)
        } else {
            let mut data = Map::new();
            let rv = func(&mut data);
            wg.insert(
                self.id.0.clone(),
                SessionInstance {
                    data,
                    expires: Instant::now().add(self.store.lifespan),
                },
            );
            rv
        }
    }

    pub fn renew(&self) {
        self.tap(|_| ())
    }

    pub fn reset(&self) {
        self.tap(|data| data.clear())
    }

    pub fn get<T: DeserializeOwned>(&self, path: &str) -> Option<T> {
        self.tap(|data| data.dot_get(path))
    }

    pub fn get_or<T: DeserializeOwned>(&self, path: &str, def: T) -> T {
        self.get(path).unwrap_or(def)
    }

    pub fn get_or_else<T: DeserializeOwned, F: FnOnce() -> T>(&self, path: &str, def: F) -> T {
        self.get(path).unwrap_or_else(def)
    }

    pub fn get_or_default<T: DeserializeOwned + Default>(&self, path: &str) -> T {
        self.get(path).unwrap_or_default()
    }

    pub fn take<T: DeserializeOwned>(&self, path: &str) -> Option<T> {
        self.tap(|data| data.dot_take(path))
    }

    pub fn replace<O: DeserializeOwned, N: Serialize>(&self, path: &str, new: N) -> Option<O> {
        self.tap(|data| data.dot_replace(path, new))
    }

    pub fn set<T: Serialize>(&self, path: &str, value: T) {
        self.tap(|data| data.dot_set(path, value));
    }

    pub fn remove(&self, path: &str) -> bool {
        self.tap(|data| data.dot_remove(path))
    }
}

/// Fairing struct
struct SessionFairing {
    lifespan: Duration
}

impl Fairing for SessionFairing {
    fn info(&self) -> Info {
        Info {
            name: "Session Fairing",
            kind: fairing::Kind::Attach | fairing::Kind::Response,
        }
    }

    fn on_attach(&self, rocket: Rocket) -> Result<Rocket, Rocket> {
        Ok(rocket.manage(SessionStore {
            inner: Default::default(),
            lifespan: self.lifespan,
        }))
    }

    fn on_response<'r>(&self, request: &'r Request, response: &mut Response) {
        let session = request.local_cache(|| SessionID("".to_string()));

        if !session.0.is_empty() {
            response.adjoin_header(Cookie::build(SESSION_ID, session.0.clone()).finish());
        }
    }
}
