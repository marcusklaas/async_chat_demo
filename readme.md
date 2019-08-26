# async chat demo

This is a demo of an asynchronous chat server usync Rust's new async functionality and the `async_std` library. The code here is mostly from the excellent [async_std tutorial](https://book.async.rs/tutorial/index.html).

## running the code

At the time of writing, async is still unstable. You will need a nightly version of rustc to compile this.

Run the server using
```bash
cargo run --bin server
```

and start a client using
```bash
cargo run --bin client
```

The first line entered into the client is used as a user name. The following lines are interpreted as messages using the following syntax:
```
login1, login2, ... loginN: message
```
