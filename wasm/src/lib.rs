use wasm_bindgen::{prelude::*, JsCast};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "[string, string]")]
    pub type TwoStrings;
}

#[wasm_bindgen]
pub fn translate(s: &str, inp_name: &str) -> Result<TwoStrings, JsValue> {
    match nix2js::translate(s, inp_name).map_err(|errors| errors.join("\n")) {
        Ok((js, map)) => Ok(JsValue::from(js_sys::Array::of2(&js.into(), &map.into()))
            .unchecked_into::<TwoStrings>()),
        Err(x) => Err(x.into()),
    }
}
