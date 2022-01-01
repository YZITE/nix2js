use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn translate(s: &str) -> Result<JsValue, JsValue> {
    match nix2js::translate(s).map_err(|errors| errors.join("\n")) {
        Ok(x) => Ok(x.into()),
        Err(x) => Err(x.into()),
    }
}
