# Sessions for Rocket.rs

Adding cookie-based sessions to a rocket application is extremely simple with this crate.

The implementation is generic to support any type as session data: a custom struct, `String`,
`HashMap`, or perhaps `serde_json::Value`. You're free to choose.

The session expiry time is configurable through the Fairing. When a session expires,
the data associated with it is dropped. All expired sessions may be cleared by calling `.remove_expired()`
on the `SessionStore`, which is be obtained in routes as `State<SessionStore>`, or from a 
session instance by calling `.get_store()`.

The session cookie is currently hardcoded to "SESSID" and contains 16 random characters.

## Basic Example

This simple example uses u64 as the session variable; note that it can be a struct, map, or anything else,
it just needs to implement `Send + Sync + Default`. 

```rust
#![feature(proc_macro_hygiene, decl_macro)]
#[macro_use] extern crate rocket;

use std::time::Duration;

// It's convenient to define a type alias:
pub type Session<'a> = rocket_session::Session<'a, u64>;

fn main() {
    rocket::ignite()
        .attach(Session::fairing(Duration::from_secs(3600)))
        .mount("/", routes![index])
        .launch();
}

#[get("/")]
fn index(session: Session) -> String {
    let count = session.tap(|n| {
        // Change the stored value (it is &mut) 
        *n += 1;

        // Return something to the caller. 
        // This can be any type, 'tap' is generic.        
        *n
    });

    format!("{} visits", count)
}
```

## Extending Session by a Trait

The `.tap()` method is powerful, but sometimes you may wish for something more convenient.

Here is an example of using a custom trait and the `json_dotpath` crate to implement
a polymorphic store based on serde serialization:

```rust
use serde_json::Value;
use serde::de::DeserializeOwned;
use serde::Serialize;
use json_dotpath::DotPaths;

pub type Session<'a> = rocket_session::Session<'a, serde_json::Map<String, Value>>;

pub trait SessionAccess {
    fn get<T: DeserializeOwned>(&self, path: &str) -> Option<T>;

    fn take<T: DeserializeOwned>(&self, path: &str) -> Option<T>;

    fn replace<O: DeserializeOwned, N: Serialize>(&self, path: &str, new: N) -> Option<O>;

    fn set<T: Serialize>(&self, path: &str, value: T);

    fn remove(&self, path: &str) -> bool;
}

impl<'a> SessionAccess for Session<'a> {
    fn get<T: DeserializeOwned>(&self, path: &str) -> Option<T> {
        self.tap(|data| data.dot_get(path))
    }

    fn take<T: DeserializeOwned>(&self, path: &str) -> Option<T> {
        self.tap(|data| data.dot_take(path))
    }

    fn replace<O: DeserializeOwned, N: Serialize>(&self, path: &str, new: N) -> Option<O> {
        self.tap(|data| data.dot_replace(path, new))
    }

    fn set<T: Serialize>(&self, path: &str, value: T) {
        self.tap(|data| data.dot_set(path, value));
    }

    fn remove(&self, path: &str) -> bool {
        self.tap(|data| data.dot_remove(path))
    }
}
```

