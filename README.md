See the documentation on the `cargo doc` for more information.

Building
```
wasm-pack build --target web --release
```

There's a simple example included, it can be viewed by using miniserve in that example folder.
```
cargo install miniserve
miniserve .
```
Then open index.html