use parking_lot::RwLock;
use rand::Rng;

use rocket::{
    fairing::{self, Fairing, Info},
    http::{Cookie, Status},
    request::FromRequest,
    Outcome, Request, Response, Rocket, State,
};

use std::collections::HashMap;
use std::marker::PhantomData;
use std::ops::Add;
use std::time::{Duration, Instant};

const SESSION_COOKIE: &str = "SESSID";
const SESSION_ID_LEN: usize = 16;

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
pub struct SessionStore<D>
    where
        D: 'static + Sync + Send + Default,
{
    /// The internaly mutable map of sessions
    inner: RwLock<HashMap<String, SessionInstance<D>>>,
    /// Sessions lifespan
    lifespan: Duration,
}

impl<D> SessionStore<D>
    where
        D: 'static + Sync + Send + Default,
{
    /// Remove all expired sessions
    pub fn remove_expired(&self) {
        let now = Instant::now();
        self.inner.write().retain(|_k, v| v.expires > now);
    }
}

/// Session ID newtype for rocket's "local_cache"
#[derive(PartialEq, Hash, Clone, Debug)]
struct SessionID(String);

impl SessionID {
    fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn to_string(&self) -> String {
        self.0.clone()
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
        let store : State<SessionStore<D>> = request.guard().unwrap();
        Outcome::Success(Session {
            id: request.local_cache(|| {
                // Resolve session ID
                let id = if let Some(cookie) = request.cookies().get(SESSION_COOKIE) {
                    SessionID(cookie.value().to_string())
                } else {
                    SessionID(
                        rand::thread_rng()
                            .sample_iter(&rand::distributions::Alphanumeric)
                            .take(SESSION_ID_LEN)
                            .collect(),
                    )
                };

                let new_expiration = Instant::now().add(store.lifespan);
                let mut wg = store.inner.write();
                match wg.get_mut(id.as_str()) {
                    Some(ses) => {
                        // Check expiration
                        if ses.expires <= Instant::now() {
                            ses.data = D::default();
                        }
                        // Update expiry timestamp
                        ses.expires = new_expiration;
                    },
                    None => {
                        // New session
                        wg.insert(
                            id.to_string(),
                            SessionInstance {
                                data: D::default(),
                                expires: new_expiration,
                            }
                        );
                    }
                };

                id
            }),
            store,
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

    /// Access the session store
    pub fn get_store(&self) -> &SessionStore<D> {
        &self.store
    }

    /// Set the session object to its default state
    pub fn reset(&self) {
        self.tap_mut(|m| {
            *m = D::default();
        })
    }

    pub fn tap<T>(&self, func: impl FnOnce(&D) -> T) -> T {
        let rg = self.store.inner.read();
        let instance = rg.get(self.id.as_str()).unwrap();
        func(&instance.data)
    }

    pub fn tap_mut<T>(&self, func: impl FnOnce(&mut D) -> T) -> T {
        let mut wg = self.store.inner.write();
        let instance = wg.get_mut(self.id.as_str()).unwrap();
        func(&mut instance.data)
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
        // install the store singleton
        Ok(rocket.manage(SessionStore::<D> {
            inner: Default::default(),
            lifespan: self.lifespan,
        }))
    }

    fn on_response<'r>(&self, request: &'r Request, response: &mut Response) {
        // send the session cookie, if session started
        let session = request.local_cache(|| SessionID("".to_string()));

        if !session.0.is_empty() {
            response.adjoin_header(Cookie::build(SESSION_COOKIE, session.0.clone()).finish());
        }
    }
}
