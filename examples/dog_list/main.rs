#![feature(proc_macro_hygiene, decl_macro)]
#[macro_use]
extern crate rocket;

use rocket::request::Form;
use rocket::response::content::Html;
use rocket::response::Redirect;

type Session<'a> = rocket_session::Session<'a, Vec<String>>;

fn main() {
    rocket::ignite()
        .attach(Session::fairing())
        .mount("/", routes![index, add, remove])
        .launch();
}

#[get("/")]
fn index(session: Session) -> Html<String> {
    let mut page = String::new();
    page.push_str(
        r#"
            <!DOCTYPE html>
            <h1>My Dogs</h1>

            <form method="POST" action="/add">
            Add Dog: <input type="text" name="name"> <input type="submit" value="Add">
            </form>

            <ul>
        "#,
    );

    session.tap(|sess| {
        for (n, dog) in sess.iter().enumerate() {
            page.push_str(&format!(
                r#"
                <li>&#x1F436; {} <a href="/remove/{}">Remove</a></li>
            "#,
                dog, n
            ));
        }
    });

    page.push_str(
        r#"
            </ul>
        "#,
    );

    Html(page)
}

#[derive(FromForm)]
struct AddForm {
    name: String,
}

#[post("/add", data = "<dog>")]
fn add(session: Session, dog: Form<AddForm>) -> Redirect {
    session.tap(move |sess| {
        sess.push(dog.into_inner().name);
    });

    Redirect::found("/")
}

#[get("/remove/<dog>")]
fn remove(session: Session, dog: usize) -> Redirect {
    session.tap(|sess| {
        if dog < sess.len() {
            sess.remove(dog);
        }
    });

    Redirect::found("/")
}