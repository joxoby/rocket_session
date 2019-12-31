# Sessions for Rocket.rs

Adding cookie-based sessions to a rocket application is extremely simple with this crate.

Sessions are used to share data between related requests, such as user authentication, shopping basket, 
form values that failed validation for re-filling, etc.

## Configuration

The implementation is generic to support any type as session data: a custom struct, `String`,
`HashMap`, or perhaps `serde_json::Value`. You're free to choose.

The session lifetime, cookie name, and other parameters can be configured by calling chained 
methods on the fairing. When a session expires, the data associated with it is dropped.

## Usage

To use session in a route, first make sure you have the fairing attached by calling
`rocket.attach(Session::fairing())` at start-up, and then add something like `session : Session` 
to the parameter list of your route(s). Everything else--session init, expiration, cookie 
management--is done for you behind the scenes.

Session data is accessed in a closure run in the session context, using the `session.tap()`
method. This closure runs inside a per-session mutex, avoiding simultaneous mutation
from different requests. Try to *avoid lengthy operations inside the closure*,
as it effectively blocks any other request to session-enabled routes by the client.

Every request to a session-enabled route extends the session's lifetime to the full 
configured time (defaults to 1 hour). Automatic clean-up removes expired sessions to make sure 
the session list does not waste memory.

## Examples

(More examples are in the examples folder)

### Basic Example

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
        .attach(Session::fairing())
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
a polymorphic store based on serde serialization. 

Note that this approach is prone to data races, since every method contains its own `.tap()`.
It may be safer to simply call the `.dot_*()` methods manually in one shared closure.

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

