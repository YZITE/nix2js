use wasm_bindgen::{prelude::*, JsCast};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "[string, string]")] // "
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

#[wasm_bindgen]
pub fn translate_inline_srcmap(s: &str, inp_name: &str) -> Result<String, JsValue> {
    match nix2js::translate(s, inp_name).map_err(|errors| errors.join("\n")) {
        Ok((mut js, map)) => Ok({
            js += "\n//# sourceMappingURL=data:application/json;charset=utf-8;base64,";
            // see also https://developer.mozilla.org/en-US/docs/Glossary/Base64#solution_2_%E2%80%93_rewriting_atob_and_btoa_using_typedarrays_and_utf-8
            js += &base64::encode(&map);
            js
        }),
        Err(x) => Err(x.into()),
    }
}
