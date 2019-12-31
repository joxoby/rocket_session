# Sessions for Rocket.rs

Adding cookie-based sessions to a rocket application is extremely simple:

```rust
#![feature(proc_macro_hygiene, decl_macro)]
#[macro_use] extern crate rocket;

use rocket_session::Session;
use std::time::Duration;

fn main() {
    rocket::ignite()
        .attach(Session::fairing(Duration::from_secs(3600)))
        .mount("/", routes![index])
        .launch();
}

#[get("/")]
fn index(session: Session) -> String {
    let mut count: usize = session.get_or_default("count");
    count += 1;
    session.set("count", count);

    format!("{} visits", count)
}
```

Anything serializable can be stored in the session, just make sure to unpack it to the right type.

The session driver internally uses `serde_json::Value` and the `json_dotpath` crate. 
Therefore, it's possible to use dotted paths and store the session data in a more structured way.

