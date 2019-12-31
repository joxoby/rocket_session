//! This demo is a page visit counter, with a custom cookie name, length, and expiry time.
//!
//! The expiry time is set to 10 seconds to illustrate how a session is cleared if inactive.

#![feature(proc_macro_hygiene, decl_macro)]
#[macro_use]
extern crate rocket;

use std::time::Duration;
use rocket::response::content::Html;

#[derive(Default, Clone)]
struct SessionData {
    visits1: usize,
    visits2: usize,
}

// It's convenient to define a type alias:
type Session<'a> = rocket_session::Session<'a, SessionData>;

fn main() {
    rocket::ignite()
        .attach(Session::fairing()
            // 10 seconds of inactivity until session expires
            // (wait 10s and refresh, the numbers will reset)
            .with_lifetime(Duration::from_secs(10))
            // custom cookie name and length
            .with_cookie_name("my_cookie")
            .with_cookie_len(20)
        )
        .mount("/", routes![index, about])
        .launch();
}

#[get("/")]
fn index(session: Session) -> Html<String> {
    // Here we build the entire response inside the 'tap' closure.

    // While inside, the session is locked to parallel changes, e.g.
    // from a different browser tab.
    session.tap(|sess| {
        sess.visits1 += 1;

        Html(format!(r##"
                <!DOCTYPE html>
                <h1>Home</h1>
                <a href="/">Refresh</a> &bull; <a href="/about/">go to About</a>
                <p>Visits: home {}, about {}</p>
            "##,
            sess.visits1,
            sess.visits2
        ))
    })
}

#[get("/about")]
fn about(session: Session) -> Html<String> {
    // Here we return a value from the tap function and use it below
    let count = session.tap(|sess| {
        sess.visits2 += 1;
        sess.visits2
    });

    Html(format!(r##"
            <!DOCTYPE html>
            <h1>About</h1>
            <a href="/about">Refresh</a> &bull; <a href="/">go home</a>
            <p>Page visits: {}</p>
        "##,
        count
    ))
}
