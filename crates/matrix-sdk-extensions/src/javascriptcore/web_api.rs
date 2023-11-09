use std::{fs::File, io::Read};

use javascriptcore::{constructor_callback, function_callback, JSException, JSTypedArrayType};

#[constructor_callback]
pub(super) fn text_encoder(
    ctx: &JSContext,
    constructor: &JSObject,
    _arguments: &[JSValue],
) -> Result<JSValue, JSException> {
    #[function_callback]
    fn encode_into(
        ctx: &JSContext,
        _function: Option<&JSObject>,
        _this_object: Option<&JSObject>,
        arguments: &[JSValue],
    ) -> Result<JSValue, JSException> {
        let string = arguments
            .get(0)
            .ok_or_else(|| -> JSException {
                JSValue::new_string(ctx, "The first argument `string` is missing").into()
            })
            .and_then(|string| {
                if string.is_string() {
                    string.as_string()
                } else {
                    Err(JSValue::new_string(ctx, "The first argument `string` is not a string")
                        .into())
                }
            })?;

        let mut utf8_array = arguments
            .get(1)
            .ok_or_else(|| -> JSException {
                JSValue::new_string(ctx, "The second argument `utf8Array` is missing").into()
            })
            .and_then(|class| {
                if class.is_typed_array()
                    && class.as_typed_array()?.ty()? == JSTypedArrayType::Uint8Array
                {
                    class.as_typed_array()
                } else {
                    Err(JSValue::new_string(
                        ctx,
                        "The second argument `uint8Array` is not a `Uint8Array`",
                    )
                    .into())
                }
            })?;

        let position = arguments
            .get(2)
            .map(|number| -> Result<usize, JSException> {
                if number.is_number() {
                    Ok(number.as_number()? as usize)
                } else {
                    Err(JSValue::new_string(ctx, "The third argument `position` is not a `number`")
                        .into())
                }
            })
            .transpose()?
            .unwrap_or_default();

        let utf8_array_slice = unsafe { utf8_array.as_mut_slice() }?;

        for (nth, char) in string.to_string().as_bytes().iter().enumerate() {
            *utf8_array_slice.get_mut(nth + position).unwrap() = *char;
        }

        let result =
            JSValue::new_from_json(ctx, "{\"read\": 0, \"written\": 0}").unwrap().as_object()?;

        let len = f64::from(string.len() as u32);
        result.set_property("read", JSValue::new_number(ctx, len))?;
        result.set_property("written", JSValue::new_number(ctx, len))?;

        Ok(result.into())
    }

    constructor
        .set_property("encodeInto", JSValue::new_function(ctx, "encodeInto", Some(encode_into)))?;

    Ok(constructor.into())
}

#[constructor_callback]
pub(super) fn text_decoder(
    ctx: &JSContext,
    constructor: &JSObject,
    _arguments: &[JSValue],
) -> Result<JSValue, JSException> {
    #[function_callback]
    fn decode(
        ctx: &JSContext,
        _function: Option<&JSObject>,
        _this_object: Option<&JSObject>,
        arguments: &[JSValue],
    ) -> Result<JSValue, JSException> {
        let utf8_array = arguments
            .get(0)
            .ok_or_else(|| -> JSException {
                JSValue::new_string(ctx, "The first argument `utf8Array` is missing").into()
            })
            .and_then(|class| {
                if class.is_typed_array()
                    && class.as_typed_array()?.ty()? == JSTypedArrayType::Uint8Array
                {
                    class.as_typed_array()
                } else {
                    Err(JSValue::new_string(
                        ctx,
                        "The first argument `uint8Array` is not a `Uint8Array`",
                    )
                    .into())
                }
            })?;

        let utf8_array_as_vec = utf8_array.to_vec()?;
        let string = String::from_utf8_lossy(&utf8_array_as_vec).into_owned();

        Ok(JSValue::new_string(ctx, string))
    }

    constructor.set_property("decode", JSValue::new_function(ctx, "decode", Some(decode)))?;

    Ok(constructor.into())
}

#[function_callback]
pub(super) fn compile_wasm(
    ctx: &JSContext,
    _function: Option<&JSObject>,
    _this_object: Option<&JSObject>,
    arguments: &[JSValue],
) -> Result<JSValue, JSException> {
    if arguments.len() != 1 {
        return Err(JSValue::new_string(ctx, "`compile` expects one argument").into());
    }

    let wasm_path = arguments[0].as_string()?.to_string();

    let mut wasm_file = File::open(format!("guests/timeline/js/{}", wasm_path)).unwrap();
    let mut wasm_bytes = Vec::new();
    wasm_file.read_to_end(&mut wasm_bytes).unwrap();

    let global_object = ctx.global_object()?;
    let webassembly = global_object.get_property("WebAssembly").as_object()?;
    let webassembly_module = webassembly.get_property("Module").as_object()?;

    let wasm_bytes_buffer =
        unsafe { JSValue::new_typed_array_with_bytes(ctx, wasm_bytes.as_mut_slice()) }?;

    let compiled_module = webassembly_module.call_as_constructor(&[wasm_bytes_buffer])?;

    Ok(compiled_module)
}

#[function_callback]
pub(super) fn instantiate_wasm(
    ctx: &JSContext,
    _function: Option<&JSObject>,
    _this_object: Option<&JSObject>,
    arguments: &[JSValue],
) -> Result<JSValue, JSException> {
    if arguments.len() > 2 {
        return Err(JSValue::new_string(ctx, "`instantiate` expects at most 2 arguments").into());
    }

    let global_object = ctx.global_object()?;
    let webassembly = global_object.get_property("WebAssembly").as_object()?;
    let webassembly_instance = webassembly.get_property("Instance").as_object()?;

    webassembly_instance.call_as_constructor(arguments)
}
