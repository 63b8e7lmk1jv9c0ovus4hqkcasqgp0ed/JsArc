use js_sys::{Array, Promise};
use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, BlobPropertyBag};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen]
    pub type HelloWorldModule;

    #[wasm_bindgen(method, js_name = "helloWorld")]
    pub fn hello_world(this: &HelloWorldModule);

}

pub async fn load_dynamic_hello_world() -> HelloWorldModule {
    let module_as_str = r#"export function helloWorld() { console.log("Hello World!"); }"#;

    let from_data = Array::new();
    from_data.push(&module_as_str.into());

    let mut type_set: BlobPropertyBag = BlobPropertyBag::new();
    type_set.type_("application/javascript");

    let blob = Blob::new_with_str_sequence_and_options(&from_data, &type_set).unwrap();
    let module_address = web_sys::Url::create_object_url_with_blob(&blob).unwrap();

    let module_promise: Promise = js_sys::eval(&format!(r#"import ("{}")"#, module_address))
        .unwrap()
        .into();

    let module = JsFuture::from(module_promise).await.unwrap();

    let as_hello_world: HelloWorldModule = module.into();
    as_hello_world.hello_world();

    as_hello_world
}
