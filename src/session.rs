use parking_lot::RwLock;
use rand::Rng;
use rocket::fairing::{self, Fairing, Info};
use rocket::request::FromRequest;

use rocket::{
    http::{Cookie, Status},
    Outcome, Request, Response, Rocket, State,
};

use serde::export::PhantomData;
use std::collections::HashMap;
use std::ops::Add;
use std::time::{Duration, Instant};

const SESSION_COOKIE: &str = "SESSID";
const SESSION_ID_LEN : usize = 16;

/// Session, as stored in the sessions store
#[derive(Debug)]
struct SessionInstance<D>
where
    D: 'static + Sync + Send + Default,
{
    /// Data object
    data: D,
    /// Expiry
    expires: Instant,
}

/// Session store (shared state)
#[derive(Default, Debug)]
struct SessionStore<D>
where
    D: 'static + Sync + Send + Default,
{
    /// The internaly mutable map of sessions
    inner: RwLock<HashMap<String, SessionInstance<D>>>,
    /// Sessions lifespan
    lifespan: Duration,
}

/// Session ID newtype for rocket's "local_cache"
#[derive(PartialEq, Hash, Clone, Debug)]
struct SessionID(String);

impl<'a, 'r> FromRequest<'a, 'r> for &'a SessionID {
    type Error = ();

    fn from_request(request: &'a Request<'r>) -> Outcome<Self, (Status, Self::Error), ()> {
        Outcome::Success(request.local_cache(|| {
            if let Some(cookie) = request.cookies().get(SESSION_COOKIE) {
                SessionID(cookie.value().to_string()) // FIXME avoid cloning (cow?)
            } else {
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

/// Session instance
#[derive(Debug)]
pub struct Session<'a, D>
where
    D: 'static + Sync + Send + Default,
{
    /// The shared state reference
    store: State<'a, SessionStore<D>>,
    /// Session ID
    id: &'a SessionID,
}

impl<'a, 'r, D> FromRequest<'a, 'r> for Session<'a, D>
where
    D: 'static + Sync + Send + Default,
{
    type Error = ();

    fn from_request(request: &'a Request<'r>) -> Outcome<Self, (Status, Self::Error), ()> {
        Outcome::Success(Session {
            id: request.local_cache(|| {
                if let Some(cookie) = request.cookies().get(SESSION_COOKIE) {
                    SessionID(cookie.value().to_string())
                } else {
                    SessionID(
                        rand::thread_rng()
                            .sample_iter(&rand::distributions::Alphanumeric)
                            .take(SESSION_ID_LEN)
                            .collect(),
                    )
                }
            }),
            store: request.guard().unwrap(),
        })
    }
}

impl<'a, D> Session<'a, D>
where
    D: 'static + Sync + Send + Default,
{
    /// Get the fairing object
    pub fn fairing(lifespan: Duration) -> impl Fairing {
        SessionFairing::<D> {
            lifespan,
            _phantom: PhantomData,
        }
    }

    /// Run a closure with a mutable reference to the session object.
    /// The closure's return value is send to the caller.
    pub fn tap<T>(&self, func: impl FnOnce(&mut D) -> T) -> T {
        let mut wg = self.store.inner.write();
        if let Some(instance) = wg.get_mut(&self.id.0) {
            instance.expires = Instant::now().add(self.store.lifespan);
            func(&mut instance.data)
        } else {
            let mut data = D::default();
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

    /// Renew the session
    pub fn renew(&self) {
        self.tap(|_| ())
    }
}

/// Fairing struct
struct SessionFairing<D>
where
    D: 'static + Sync + Send + Default,
{
    lifespan: Duration,
    _phantom: PhantomData<D>,
}

impl<D> Fairing for SessionFairing<D>
where
    D: 'static + Sync + Send + Default,
{
    fn info(&self) -> Info {
        Info {
            name: "Session",
            kind: fairing::Kind::Attach | fairing::Kind::Response,
        }
    }

    fn on_attach(&self, rocket: Rocket) -> Result<Rocket, Rocket> {
        Ok(rocket.manage(SessionStore::<D> {
            inner: Default::default(),
            lifespan: self.lifespan,
        }))
    }

    fn on_response<'r>(&self, request: &'r Request, response: &mut Response) {
        let session = request.local_cache(|| SessionID("".to_string()));

        if !session.0.is_empty() {
            response.adjoin_header(Cookie::build(SESSION_COOKIE, session.0.clone()).finish());
        }
    }
}
