use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn translate(s: &str, inp_name: &str) -> Result<JsValue, JsValue> {
    match nix2js::translate(s, inp_name).map_err(|errors| errors.join("\n")) {
        Ok((js, map)) => Ok(js_sys::Array::of2(&js.into(), &map.into()).into()),
        Err(x) => Err(x.into()),
    }
}
