use crate::{
    add, arora_generated, cos, fail, fallback, increase, is_str_set, parallel, regex_match, run,
    seq, seq_star, set_str, status_identity, store, succeed, unset_str, wait_str_set,
};
use arora_buffers::*;
#[doc = "is_str_set"]
#[no_mangle]
pub extern "C" fn arora_function_20ba3f0f_309e_4cd2_adfc_aca6cc432526(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &IS_STR_SET_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        let mut param_value_c4f1e72d30fe400ba584f08e93944026: Option<String> = None;
        for _ in 0..field_count {
            let field_raw_id = reader.get_structure_field();
            if field_raw_id == IS_STR_SET_VALUE_PARAMETER_RAW_ID {
                param_value_c4f1e72d30fe400ba584f08e93944026 = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_STRING) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_STRING, _next_type
                            ));
                        }
                    }
                    reader.get_string().to_string()
                });
            } else {
                return Err(format!("unexpected parameter {:?}", field_raw_id));
            }
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&IS_STR_SET_FUNCTION_RAW_ID, (0usize + 1) as u32);
        writer.add_structure_field(&IS_STR_SET_FUNCTION_RAW_ID);
        let result = is_str_set(param_value_c4f1e72d30fe400ba584f08e93944026);
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "succeed"]
#[no_mangle]
pub extern "C" fn arora_function_6696f0bd_e781_40cd_aeb5_8dc616f810d2(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &SUCCEED_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        if field_count != 0 {
            return Err(format!("expected 0 parameters but got {}", field_count));
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&SUCCEED_FUNCTION_RAW_ID, (0usize + 1) as u32);
        writer.add_structure_field(&SUCCEED_FUNCTION_RAW_ID);
        let result = succeed();
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "unset_str"]
#[no_mangle]
pub extern "C" fn arora_function_7dce01ed_9818_4b7d_b45a_2e7fdece3633(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &UNSET_STR_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        let mut param_variable_2c84bf0f4ec241a483ee3f92a53be79d: Option<String> = None;
        for _ in 0..field_count {
            let field_raw_id = reader.get_structure_field();
            if field_raw_id == UNSET_STR_VARIABLE_PARAMETER_RAW_ID {
                param_variable_2c84bf0f4ec241a483ee3f92a53be79d = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_STRING) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_STRING, _next_type
                            ));
                        }
                    }
                    reader.get_string().to_string()
                });
            } else {
                return Err(format!("unexpected parameter {:?}", field_raw_id));
            }
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&UNSET_STR_FUNCTION_RAW_ID, (1usize + 1) as u32);
        writer.add_structure_field(&UNSET_STR_FUNCTION_RAW_ID);
        let result = unset_str(&mut param_variable_2c84bf0f4ec241a483ee3f92a53be79d);
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        writer.add_structure_field(&UNSET_STR_VARIABLE_PARAMETER_RAW_ID);
        writer.add_string(
            param_variable_2c84bf0f4ec241a483ee3f92a53be79d
                .unwrap()
                .as_str(),
        );
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "add"]
#[no_mangle]
pub extern "C" fn arora_function_65be1fe9_ac2a_4b6e_8870_68ac7bde6f0a(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &ADD_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        let mut param_b_cbb21d3d69b1488ba3c8236ca68263ae: Option<f32> = None;
        let mut param_res_13d7a1c22d374d0eb3172924671d2210: Option<f32> = None;
        let mut param_a_0b8885b0afca4378abe679e2ff0ee72b: Option<f32> = None;
        for _ in 0..field_count {
            let field_raw_id = reader.get_structure_field();
            if field_raw_id == ADD_B_PARAMETER_RAW_ID {
                param_b_cbb21d3d69b1488ba3c8236ca68263ae = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_F32) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_F32, _next_type
                            ));
                        }
                    }
                    reader.get_f32()
                });
            } else if field_raw_id == ADD_RES_PARAMETER_RAW_ID {
                param_res_13d7a1c22d374d0eb3172924671d2210 = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_F32) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_F32, _next_type
                            ));
                        }
                    }
                    reader.get_f32()
                });
            } else if field_raw_id == ADD_A_PARAMETER_RAW_ID {
                param_a_0b8885b0afca4378abe679e2ff0ee72b = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_F32) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_F32, _next_type
                            ));
                        }
                    }
                    reader.get_f32()
                });
            } else {
                return Err(format!("unexpected parameter {:?}", field_raw_id));
            }
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&ADD_FUNCTION_RAW_ID, (1usize + 1) as u32);
        writer.add_structure_field(&ADD_FUNCTION_RAW_ID);
        let result = add(
            param_a_0b8885b0afca4378abe679e2ff0ee72b,
            param_b_cbb21d3d69b1488ba3c8236ca68263ae,
            &mut param_res_13d7a1c22d374d0eb3172924671d2210,
        );
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        writer.add_structure_field(&ADD_RES_PARAMETER_RAW_ID);
        writer.add_f32(param_res_13d7a1c22d374d0eb3172924671d2210.unwrap());
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "fallback"]
#[no_mangle]
pub extern "C" fn arora_function_bfa89a4e_c369_430e_be78_0dc07311391c(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &FALLBACK_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        let mut param_children_5b6e9515dbcc411dbee93d8cba5fedda: Option<
            Vec<arora_generated::behavior_tree::tick_id::TickId>,
        > = None;
        for _ in 0..field_count {
            let field_raw_id = reader.get_structure_field();
            if field_raw_id == FALLBACK_CHILDREN_PARAMETER_RAW_ID {
                param_children_5b6e9515dbcc411dbee93d8cba5fedda = Some({
                    {
                        let _at = reader.next_type();
                        if _at != Some(TYPE_ARRAY) {
                            return Err(format!("expected array, got {:?}", _at));
                        }
                    }
                    let (ty, count) = reader.get_array();
                    if ty != TYPE_STRUCTURE {
                        return Err(format!(
                            "expected array element type {:?}, got {:?}",
                            TYPE_STRUCTURE, ty
                        ));
                    }
                    {
                        let _id = reader.get_structure_field();
                        if _id
                            != &[
                                0x6f, 0x49, 0xe6, 0x50, 0x84, 0xca, 0x48, 0x99, 0xa9, 0xbd, 0x1f,
                                0x3b, 0xf1, 0x7f, 0xab, 0x51,
                            ]
                        {
                            return Err("array type id mismatch".to_string());
                        }
                    }
                    let mut res =
                        Vec::<arora_generated::behavior_tree::tick_id::TickId>::with_capacity(
                            count as usize,
                        );
                    for _i in 0..count {
                        res.push(
                            arora_generated::behavior_tree::tick_id::deserialize_from_reader(
                                &mut reader,
                                false,
                            )
                            .expect(&format!(
                                "failed to deserialize {}",
                                "arora_generated :: behavior_tree :: tick_id :: TickId"
                            )),
                        );
                    }
                    res
                });
            } else {
                return Err(format!("unexpected parameter {:?}", field_raw_id));
            }
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&FALLBACK_FUNCTION_RAW_ID, (0usize + 1) as u32);
        writer.add_structure_field(&FALLBACK_FUNCTION_RAW_ID);
        let result = fallback(param_children_5b6e9515dbcc411dbee93d8cba5fedda);
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "cos"]
#[no_mangle]
pub extern "C" fn arora_function_104b9710_5d43_4a93_944c_d64bddb30ef8(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &COS_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        let mut param_angle_272fbafdc2a54ffea2949cabe6e6c1e7: Option<f32> = None;
        let mut param_res_1d10168605d847b49292fdc9e5a0daeb: Option<f32> = None;
        for _ in 0..field_count {
            let field_raw_id = reader.get_structure_field();
            if field_raw_id == COS_ANGLE_PARAMETER_RAW_ID {
                param_angle_272fbafdc2a54ffea2949cabe6e6c1e7 = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_F32) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_F32, _next_type
                            ));
                        }
                    }
                    reader.get_f32()
                });
            } else if field_raw_id == COS_RES_PARAMETER_RAW_ID {
                param_res_1d10168605d847b49292fdc9e5a0daeb = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_F32) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_F32, _next_type
                            ));
                        }
                    }
                    reader.get_f32()
                });
            } else {
                return Err(format!("unexpected parameter {:?}", field_raw_id));
            }
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&COS_FUNCTION_RAW_ID, (1usize + 1) as u32);
        writer.add_structure_field(&COS_FUNCTION_RAW_ID);
        let result = cos(
            param_angle_272fbafdc2a54ffea2949cabe6e6c1e7,
            &mut param_res_1d10168605d847b49292fdc9e5a0daeb,
        );
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        writer.add_structure_field(&COS_RES_PARAMETER_RAW_ID);
        writer.add_f32(param_res_1d10168605d847b49292fdc9e5a0daeb.unwrap());
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "store"]
#[no_mangle]
pub extern "C" fn arora_function_b8349b96_abc7_4a31_906c_da1ce6fa356e(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &STORE_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        let mut param_storage_2345a3a5a80d448099273c65bd2b7543: Option<f32> = None;
        let mut param_value_0a0778cdcb7a41fc96d4512cc8538ce2: Option<f32> = None;
        for _ in 0..field_count {
            let field_raw_id = reader.get_structure_field();
            if field_raw_id == STORE_STORAGE_PARAMETER_RAW_ID {
                param_storage_2345a3a5a80d448099273c65bd2b7543 = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_F32) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_F32, _next_type
                            ));
                        }
                    }
                    reader.get_f32()
                });
            } else if field_raw_id == STORE_VALUE_PARAMETER_RAW_ID {
                param_value_0a0778cdcb7a41fc96d4512cc8538ce2 = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_F32) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_F32, _next_type
                            ));
                        }
                    }
                    reader.get_f32()
                });
            } else {
                return Err(format!("unexpected parameter {:?}", field_raw_id));
            }
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&STORE_FUNCTION_RAW_ID, (1usize + 1) as u32);
        writer.add_structure_field(&STORE_FUNCTION_RAW_ID);
        let result = store(
            &mut param_storage_2345a3a5a80d448099273c65bd2b7543,
            param_value_0a0778cdcb7a41fc96d4512cc8538ce2,
        );
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        writer.add_structure_field(&STORE_STORAGE_PARAMETER_RAW_ID);
        writer.add_f32(param_storage_2345a3a5a80d448099273c65bd2b7543.unwrap());
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "increase"]
#[no_mangle]
pub extern "C" fn arora_function_7f6fc4a9_567c_4f15_87cc_7ca34ae1456f(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &INCREASE_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        let mut param_delta_1018eb852d044995a349b6c83c27f287: Option<f32> = None;
        let mut param_storage_e898fe88cc6146d2aeccb4fc0beb862f: Option<f32> = None;
        for _ in 0..field_count {
            let field_raw_id = reader.get_structure_field();
            if field_raw_id == INCREASE_DELTA_PARAMETER_RAW_ID {
                param_delta_1018eb852d044995a349b6c83c27f287 = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_F32) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_F32, _next_type
                            ));
                        }
                    }
                    reader.get_f32()
                });
            } else if field_raw_id == INCREASE_STORAGE_PARAMETER_RAW_ID {
                param_storage_e898fe88cc6146d2aeccb4fc0beb862f = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_F32) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_F32, _next_type
                            ));
                        }
                    }
                    reader.get_f32()
                });
            } else {
                return Err(format!("unexpected parameter {:?}", field_raw_id));
            }
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&INCREASE_FUNCTION_RAW_ID, (1usize + 1) as u32);
        writer.add_structure_field(&INCREASE_FUNCTION_RAW_ID);
        let result = increase(
            &mut param_storage_e898fe88cc6146d2aeccb4fc0beb862f,
            param_delta_1018eb852d044995a349b6c83c27f287,
        );
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        writer.add_structure_field(&INCREASE_STORAGE_PARAMETER_RAW_ID);
        writer.add_f32(param_storage_e898fe88cc6146d2aeccb4fc0beb862f.unwrap());
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "seq_star"]
#[no_mangle]
pub extern "C" fn arora_function_c2d5ed72_798c_4174_94f7_13378bd9bf1f(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &SEQ_STAR_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        let mut param_current_index_4de502df3f48454194d8dd68fe92bc8e: Option<u16> = None;
        let mut param_children_5b6e9515dbcc411dbee93d8cba5fedda: Option<
            Vec<arora_generated::behavior_tree::tick_id::TickId>,
        > = None;
        for _ in 0..field_count {
            let field_raw_id = reader.get_structure_field();
            if field_raw_id == SEQ_STAR_CURRENT_INDEX_PARAMETER_RAW_ID {
                param_current_index_4de502df3f48454194d8dd68fe92bc8e = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_U16) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_U16, _next_type
                            ));
                        }
                    }
                    reader.get_u16()
                });
            } else if field_raw_id == SEQ_STAR_CHILDREN_PARAMETER_RAW_ID {
                param_children_5b6e9515dbcc411dbee93d8cba5fedda = Some({
                    {
                        let _at = reader.next_type();
                        if _at != Some(TYPE_ARRAY) {
                            return Err(format!("expected array, got {:?}", _at));
                        }
                    }
                    let (ty, count) = reader.get_array();
                    if ty != TYPE_STRUCTURE {
                        return Err(format!(
                            "expected array element type {:?}, got {:?}",
                            TYPE_STRUCTURE, ty
                        ));
                    }
                    {
                        let _id = reader.get_structure_field();
                        if _id
                            != &[
                                0x6f, 0x49, 0xe6, 0x50, 0x84, 0xca, 0x48, 0x99, 0xa9, 0xbd, 0x1f,
                                0x3b, 0xf1, 0x7f, 0xab, 0x51,
                            ]
                        {
                            return Err("array type id mismatch".to_string());
                        }
                    }
                    let mut res =
                        Vec::<arora_generated::behavior_tree::tick_id::TickId>::with_capacity(
                            count as usize,
                        );
                    for _i in 0..count {
                        res.push(
                            arora_generated::behavior_tree::tick_id::deserialize_from_reader(
                                &mut reader,
                                false,
                            )
                            .expect(&format!(
                                "failed to deserialize {}",
                                "arora_generated :: behavior_tree :: tick_id :: TickId"
                            )),
                        );
                    }
                    res
                });
            } else {
                return Err(format!("unexpected parameter {:?}", field_raw_id));
            }
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&SEQ_STAR_FUNCTION_RAW_ID, (1usize + 1) as u32);
        writer.add_structure_field(&SEQ_STAR_FUNCTION_RAW_ID);
        let result = seq_star(
            param_children_5b6e9515dbcc411dbee93d8cba5fedda,
            &mut param_current_index_4de502df3f48454194d8dd68fe92bc8e,
        );
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        writer.add_structure_field(&SEQ_STAR_CURRENT_INDEX_PARAMETER_RAW_ID);
        writer.add_u16(param_current_index_4de502df3f48454194d8dd68fe92bc8e.unwrap());
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "seq"]
#[no_mangle]
pub extern "C" fn arora_function_32246df6_ab5d_4f18_9221_23e28731de93(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &SEQ_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        let mut param_children_5b6e9515dbcc411dbee93d8cba5fedda: Option<
            Vec<arora_generated::behavior_tree::tick_id::TickId>,
        > = None;
        for _ in 0..field_count {
            let field_raw_id = reader.get_structure_field();
            if field_raw_id == SEQ_CHILDREN_PARAMETER_RAW_ID {
                param_children_5b6e9515dbcc411dbee93d8cba5fedda = Some({
                    {
                        let _at = reader.next_type();
                        if _at != Some(TYPE_ARRAY) {
                            return Err(format!("expected array, got {:?}", _at));
                        }
                    }
                    let (ty, count) = reader.get_array();
                    if ty != TYPE_STRUCTURE {
                        return Err(format!(
                            "expected array element type {:?}, got {:?}",
                            TYPE_STRUCTURE, ty
                        ));
                    }
                    {
                        let _id = reader.get_structure_field();
                        if _id
                            != &[
                                0x6f, 0x49, 0xe6, 0x50, 0x84, 0xca, 0x48, 0x99, 0xa9, 0xbd, 0x1f,
                                0x3b, 0xf1, 0x7f, 0xab, 0x51,
                            ]
                        {
                            return Err("array type id mismatch".to_string());
                        }
                    }
                    let mut res =
                        Vec::<arora_generated::behavior_tree::tick_id::TickId>::with_capacity(
                            count as usize,
                        );
                    for _i in 0..count {
                        res.push(
                            arora_generated::behavior_tree::tick_id::deserialize_from_reader(
                                &mut reader,
                                false,
                            )
                            .expect(&format!(
                                "failed to deserialize {}",
                                "arora_generated :: behavior_tree :: tick_id :: TickId"
                            )),
                        );
                    }
                    res
                });
            } else {
                return Err(format!("unexpected parameter {:?}", field_raw_id));
            }
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&SEQ_FUNCTION_RAW_ID, (0usize + 1) as u32);
        writer.add_structure_field(&SEQ_FUNCTION_RAW_ID);
        let result = seq(param_children_5b6e9515dbcc411dbee93d8cba5fedda);
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "parallel"]
#[no_mangle]
pub extern "C" fn arora_function_a9340289_1f30_411f_9faa_0f07d54613e8(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &PARALLEL_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        let mut param_children_5b6e9515dbcc411dbee93d8cba5fedda: Option<
            Vec<arora_generated::behavior_tree::tick_id::TickId>,
        > = None;
        for _ in 0..field_count {
            let field_raw_id = reader.get_structure_field();
            if field_raw_id == PARALLEL_CHILDREN_PARAMETER_RAW_ID {
                param_children_5b6e9515dbcc411dbee93d8cba5fedda = Some({
                    {
                        let _at = reader.next_type();
                        if _at != Some(TYPE_ARRAY) {
                            return Err(format!("expected array, got {:?}", _at));
                        }
                    }
                    let (ty, count) = reader.get_array();
                    if ty != TYPE_STRUCTURE {
                        return Err(format!(
                            "expected array element type {:?}, got {:?}",
                            TYPE_STRUCTURE, ty
                        ));
                    }
                    {
                        let _id = reader.get_structure_field();
                        if _id
                            != &[
                                0x6f, 0x49, 0xe6, 0x50, 0x84, 0xca, 0x48, 0x99, 0xa9, 0xbd, 0x1f,
                                0x3b, 0xf1, 0x7f, 0xab, 0x51,
                            ]
                        {
                            return Err("array type id mismatch".to_string());
                        }
                    }
                    let mut res =
                        Vec::<arora_generated::behavior_tree::tick_id::TickId>::with_capacity(
                            count as usize,
                        );
                    for _i in 0..count {
                        res.push(
                            arora_generated::behavior_tree::tick_id::deserialize_from_reader(
                                &mut reader,
                                false,
                            )
                            .expect(&format!(
                                "failed to deserialize {}",
                                "arora_generated :: behavior_tree :: tick_id :: TickId"
                            )),
                        );
                    }
                    res
                });
            } else {
                return Err(format!("unexpected parameter {:?}", field_raw_id));
            }
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&PARALLEL_FUNCTION_RAW_ID, (0usize + 1) as u32);
        writer.add_structure_field(&PARALLEL_FUNCTION_RAW_ID);
        let result = parallel(param_children_5b6e9515dbcc411dbee93d8cba5fedda);
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "set_str"]
#[no_mangle]
pub extern "C" fn arora_function_c803889f_4757_4b56_908f_4b2b47041eff(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &SET_STR_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        let mut param_value_88438955787244ad8464d636dc5fe26f: Option<String> = None;
        let mut param_variable_8fa2f9651eb540d9baca8facef0d31a8: Option<String> = None;
        for _ in 0..field_count {
            let field_raw_id = reader.get_structure_field();
            if field_raw_id == SET_STR_VALUE_PARAMETER_RAW_ID {
                param_value_88438955787244ad8464d636dc5fe26f = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_STRING) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_STRING, _next_type
                            ));
                        }
                    }
                    reader.get_string().to_string()
                });
            } else if field_raw_id == SET_STR_VARIABLE_PARAMETER_RAW_ID {
                param_variable_8fa2f9651eb540d9baca8facef0d31a8 = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_STRING) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_STRING, _next_type
                            ));
                        }
                    }
                    reader.get_string().to_string()
                });
            } else {
                return Err(format!("unexpected parameter {:?}", field_raw_id));
            }
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&SET_STR_FUNCTION_RAW_ID, (1usize + 1) as u32);
        writer.add_structure_field(&SET_STR_FUNCTION_RAW_ID);
        let result = set_str(
            &mut param_variable_8fa2f9651eb540d9baca8facef0d31a8,
            param_value_88438955787244ad8464d636dc5fe26f,
        );
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        writer.add_structure_field(&SET_STR_VARIABLE_PARAMETER_RAW_ID);
        writer.add_string(
            param_variable_8fa2f9651eb540d9baca8facef0d31a8
                .unwrap()
                .as_str(),
        );
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "regex_match"]
#[no_mangle]
pub extern "C" fn arora_function_8e3dbcc1_1a81_4cf6_a457_6e0c075456fd(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &REGEX_MATCH_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        let mut param_value_3267f0938a7f4b77b74c3bd2e7ad40f9: Option<String> = None;
        let mut param_first_match_e8b71df72bb544988bc3833c5bc8eadc: Option<String> = None;
        let mut param_matcher_6702e02df6ba4c5dacab9ade0a690afa: Option<String> = None;
        for _ in 0..field_count {
            let field_raw_id = reader.get_structure_field();
            if field_raw_id == REGEX_MATCH_VALUE_PARAMETER_RAW_ID {
                param_value_3267f0938a7f4b77b74c3bd2e7ad40f9 = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_STRING) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_STRING, _next_type
                            ));
                        }
                    }
                    reader.get_string().to_string()
                });
            } else if field_raw_id == REGEX_MATCH_FIRST_MATCH_PARAMETER_RAW_ID {
                param_first_match_e8b71df72bb544988bc3833c5bc8eadc = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_STRING) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_STRING, _next_type
                            ));
                        }
                    }
                    reader.get_string().to_string()
                });
            } else if field_raw_id == REGEX_MATCH_MATCHER_PARAMETER_RAW_ID {
                param_matcher_6702e02df6ba4c5dacab9ade0a690afa = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_STRING) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_STRING, _next_type
                            ));
                        }
                    }
                    reader.get_string().to_string()
                });
            } else {
                return Err(format!("unexpected parameter {:?}", field_raw_id));
            }
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&REGEX_MATCH_FUNCTION_RAW_ID, (1usize + 1) as u32);
        writer.add_structure_field(&REGEX_MATCH_FUNCTION_RAW_ID);
        let result = regex_match(
            param_value_3267f0938a7f4b77b74c3bd2e7ad40f9,
            param_matcher_6702e02df6ba4c5dacab9ade0a690afa,
            &mut param_first_match_e8b71df72bb544988bc3833c5bc8eadc,
        );
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        writer.add_structure_field(&REGEX_MATCH_FIRST_MATCH_PARAMETER_RAW_ID);
        writer.add_string(
            param_first_match_e8b71df72bb544988bc3833c5bc8eadc
                .unwrap()
                .as_str(),
        );
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "status_identity"]
#[no_mangle]
pub extern "C" fn arora_function_ef48e6d3_c735_4b5c_8f63_fc54d94dd4ee(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &STATUS_IDENTITY_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        let mut param_value_e1f174e6ca9e434484cb7f3f22115239: Option<
            arora_generated::behavior_tree::status::Status,
        > = None;
        for _ in 0..field_count {
            let field_raw_id = reader.get_structure_field();
            if field_raw_id == STATUS_IDENTITY_VALUE_PARAMETER_RAW_ID {
                param_value_e1f174e6ca9e434484cb7f3f22115239 = Some(
                    arora_generated::behavior_tree::status::deserialize_from_reader(
                        &mut reader,
                        true,
                    )
                    .map_err(|e| {
                        format!(
                            "failed to deserialize {}: {}",
                            "arora_generated :: behavior_tree :: status :: Status", e
                        )
                    })?,
                );
            } else {
                return Err(format!("unexpected parameter {:?}", field_raw_id));
            }
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&STATUS_IDENTITY_FUNCTION_RAW_ID, (0usize + 1) as u32);
        writer.add_structure_field(&STATUS_IDENTITY_FUNCTION_RAW_ID);
        let result = status_identity(param_value_e1f174e6ca9e434484cb7f3f22115239);
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "run"]
#[no_mangle]
pub extern "C" fn arora_function_41ae5ed0_1d12_4b71_aab8_02e7efedf177(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &RUN_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        if field_count != 0 {
            return Err(format!("expected 0 parameters but got {}", field_count));
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&RUN_FUNCTION_RAW_ID, (0usize + 1) as u32);
        writer.add_structure_field(&RUN_FUNCTION_RAW_ID);
        let result = run();
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "fail"]
#[no_mangle]
pub extern "C" fn arora_function_3abbbfb6_d00d_41eb_88bb_97874267eaf6(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &FAIL_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        if field_count != 0 {
            return Err(format!("expected 0 parameters but got {}", field_count));
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&FAIL_FUNCTION_RAW_ID, (0usize + 1) as u32);
        writer.add_structure_field(&FAIL_FUNCTION_RAW_ID);
        let result = fail();
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "wait_str_set"]
#[no_mangle]
pub extern "C" fn arora_function_3180977c_25a1_458e_ab82_11f36c654518(input_addr: usize) -> usize {
    let input_ptr = input_addr as *const u8;
    const INPUT_SIZE_SIZE: usize = std::mem::size_of::<u32>();
    let input_size_bytes: &[u8; 4] =
        unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE) }
            .try_into()
            .expect("input is too small");
    let input_size = u32::from_le_bytes(*input_size_bytes) as usize;
    let input = unsafe { std::slice::from_raw_parts(input_ptr, INPUT_SIZE_SIZE + input_size) };
    let _result: ::std::result::Result<::std::boxed::Box<[u8]>, ::std::string::String> = (|| {
        let mut reader = BufferReader::new(&input);
        let type_raw_id_opt = reader.next_type();
        if type_raw_id_opt.is_none() {
            return Err("input is empty".to_string());
        }
        if type_raw_id_opt.unwrap() != TYPE_STRUCTURE {
            return Err(format!(
                "expected structure input, got type {:?}",
                type_raw_id_opt
            ));
        }
        let (structure_raw_id, field_count) = reader.get_structure();
        if structure_raw_id != &WAIT_STR_SET_FUNCTION_RAW_ID {
            return Err("function id mismatch in input".to_string());
        }
        let mut param_value_8f190079e51944d3ac363bfc322e87eb: Option<String> = None;
        for _ in 0..field_count {
            let field_raw_id = reader.get_structure_field();
            if field_raw_id == WAIT_STR_SET_VALUE_PARAMETER_RAW_ID {
                param_value_8f190079e51944d3ac363bfc322e87eb = Some({
                    {
                        let _next_type = reader.next_type();
                        if _next_type != Some(TYPE_STRING) {
                            return Err(format!(
                                "type mismatch: expected {:?} but got {:?}",
                                TYPE_STRING, _next_type
                            ));
                        }
                    }
                    reader.get_string().to_string()
                });
            } else {
                return Err(format!("unexpected parameter {:?}", field_raw_id));
            }
        }
        let mut writer = BufferWriter::new();
        writer.begin_structure(&WAIT_STR_SET_FUNCTION_RAW_ID, (0usize + 1) as u32);
        writer.add_structure_field(&WAIT_STR_SET_FUNCTION_RAW_ID);
        let result = wait_str_set(param_value_8f190079e51944d3ac363bfc322e87eb);
        arora_generated::behavior_tree::status::serialize_to_writer(&result, &mut writer);
        ::std::result::Result::Ok(writer.finalize())
    })();
    match _result {
        ::std::result::Result::Ok(buf) => ::std::boxed::Box::leak(buf).as_ptr() as usize,
        ::std::result::Result::Err(msg) => {
            let mut writer = BufferWriter::new();
            writer.add_error(&msg);
            ::std::boxed::Box::leak(writer.finalize()).as_ptr() as usize
        }
    }
}
#[doc = "is_str_set: 20ba3f0f-309e-4cd2-adfc-aca6cc432526"]
pub const IS_STR_SET_FUNCTION_RAW_ID: [u8; 16] = [
    0x20, 0xba, 0x3f, 0x0f, 0x30, 0x9e, 0x4c, 0xd2, 0xad, 0xfc, 0xac, 0xa6, 0xcc, 0x43, 0x25, 0x26,
];
#[doc = "is_str_set.value: c4f1e72d-30fe-400b-a584-f08e93944026"]
pub const IS_STR_SET_VALUE_PARAMETER_RAW_ID: [u8; 16] = [
    0xc4, 0xf1, 0xe7, 0x2d, 0x30, 0xfe, 0x40, 0x0b, 0xa5, 0x84, 0xf0, 0x8e, 0x93, 0x94, 0x40, 0x26,
];
#[doc = "succeed: 6696f0bd-e781-40cd-aeb5-8dc616f810d2"]
pub const SUCCEED_FUNCTION_RAW_ID: [u8; 16] = [
    0x66, 0x96, 0xf0, 0xbd, 0xe7, 0x81, 0x40, 0xcd, 0xae, 0xb5, 0x8d, 0xc6, 0x16, 0xf8, 0x10, 0xd2,
];
#[doc = "unset_str: 7dce01ed-9818-4b7d-b45a-2e7fdece3633"]
pub const UNSET_STR_FUNCTION_RAW_ID: [u8; 16] = [
    0x7d, 0xce, 0x01, 0xed, 0x98, 0x18, 0x4b, 0x7d, 0xb4, 0x5a, 0x2e, 0x7f, 0xde, 0xce, 0x36, 0x33,
];
#[doc = "unset_str.variable: 2c84bf0f-4ec2-41a4-83ee-3f92a53be79d"]
pub const UNSET_STR_VARIABLE_PARAMETER_RAW_ID: [u8; 16] = [
    0x2c, 0x84, 0xbf, 0x0f, 0x4e, 0xc2, 0x41, 0xa4, 0x83, 0xee, 0x3f, 0x92, 0xa5, 0x3b, 0xe7, 0x9d,
];
#[doc = "add: 65be1fe9-ac2a-4b6e-8870-68ac7bde6f0a"]
pub const ADD_FUNCTION_RAW_ID: [u8; 16] = [
    0x65, 0xbe, 0x1f, 0xe9, 0xac, 0x2a, 0x4b, 0x6e, 0x88, 0x70, 0x68, 0xac, 0x7b, 0xde, 0x6f, 0x0a,
];
#[doc = "add.b: cbb21d3d-69b1-488b-a3c8-236ca68263ae"]
pub const ADD_B_PARAMETER_RAW_ID: [u8; 16] = [
    0xcb, 0xb2, 0x1d, 0x3d, 0x69, 0xb1, 0x48, 0x8b, 0xa3, 0xc8, 0x23, 0x6c, 0xa6, 0x82, 0x63, 0xae,
];
#[doc = "add.res: 13d7a1c2-2d37-4d0e-b317-2924671d2210"]
pub const ADD_RES_PARAMETER_RAW_ID: [u8; 16] = [
    0x13, 0xd7, 0xa1, 0xc2, 0x2d, 0x37, 0x4d, 0x0e, 0xb3, 0x17, 0x29, 0x24, 0x67, 0x1d, 0x22, 0x10,
];
#[doc = "add.a: 0b8885b0-afca-4378-abe6-79e2ff0ee72b"]
pub const ADD_A_PARAMETER_RAW_ID: [u8; 16] = [
    0x0b, 0x88, 0x85, 0xb0, 0xaf, 0xca, 0x43, 0x78, 0xab, 0xe6, 0x79, 0xe2, 0xff, 0x0e, 0xe7, 0x2b,
];
#[doc = "fallback: bfa89a4e-c369-430e-be78-0dc07311391c"]
pub const FALLBACK_FUNCTION_RAW_ID: [u8; 16] = [
    0xbf, 0xa8, 0x9a, 0x4e, 0xc3, 0x69, 0x43, 0x0e, 0xbe, 0x78, 0x0d, 0xc0, 0x73, 0x11, 0x39, 0x1c,
];
#[doc = "fallback.children: 5b6e9515-dbcc-411d-bee9-3d8cba5fedda"]
pub const FALLBACK_CHILDREN_PARAMETER_RAW_ID: [u8; 16] = [
    0x5b, 0x6e, 0x95, 0x15, 0xdb, 0xcc, 0x41, 0x1d, 0xbe, 0xe9, 0x3d, 0x8c, 0xba, 0x5f, 0xed, 0xda,
];
#[doc = "cos: 104b9710-5d43-4a93-944c-d64bddb30ef8"]
pub const COS_FUNCTION_RAW_ID: [u8; 16] = [
    0x10, 0x4b, 0x97, 0x10, 0x5d, 0x43, 0x4a, 0x93, 0x94, 0x4c, 0xd6, 0x4b, 0xdd, 0xb3, 0x0e, 0xf8,
];
#[doc = "cos.angle: 272fbafd-c2a5-4ffe-a294-9cabe6e6c1e7"]
pub const COS_ANGLE_PARAMETER_RAW_ID: [u8; 16] = [
    0x27, 0x2f, 0xba, 0xfd, 0xc2, 0xa5, 0x4f, 0xfe, 0xa2, 0x94, 0x9c, 0xab, 0xe6, 0xe6, 0xc1, 0xe7,
];
#[doc = "cos.res: 1d101686-05d8-47b4-9292-fdc9e5a0daeb"]
pub const COS_RES_PARAMETER_RAW_ID: [u8; 16] = [
    0x1d, 0x10, 0x16, 0x86, 0x05, 0xd8, 0x47, 0xb4, 0x92, 0x92, 0xfd, 0xc9, 0xe5, 0xa0, 0xda, 0xeb,
];
#[doc = "store: b8349b96-abc7-4a31-906c-da1ce6fa356e"]
pub const STORE_FUNCTION_RAW_ID: [u8; 16] = [
    0xb8, 0x34, 0x9b, 0x96, 0xab, 0xc7, 0x4a, 0x31, 0x90, 0x6c, 0xda, 0x1c, 0xe6, 0xfa, 0x35, 0x6e,
];
#[doc = "store.storage: 2345a3a5-a80d-4480-9927-3c65bd2b7543"]
pub const STORE_STORAGE_PARAMETER_RAW_ID: [u8; 16] = [
    0x23, 0x45, 0xa3, 0xa5, 0xa8, 0x0d, 0x44, 0x80, 0x99, 0x27, 0x3c, 0x65, 0xbd, 0x2b, 0x75, 0x43,
];
#[doc = "store.value: 0a0778cd-cb7a-41fc-96d4-512cc8538ce2"]
pub const STORE_VALUE_PARAMETER_RAW_ID: [u8; 16] = [
    0x0a, 0x07, 0x78, 0xcd, 0xcb, 0x7a, 0x41, 0xfc, 0x96, 0xd4, 0x51, 0x2c, 0xc8, 0x53, 0x8c, 0xe2,
];
#[doc = "increase: 7f6fc4a9-567c-4f15-87cc-7ca34ae1456f"]
pub const INCREASE_FUNCTION_RAW_ID: [u8; 16] = [
    0x7f, 0x6f, 0xc4, 0xa9, 0x56, 0x7c, 0x4f, 0x15, 0x87, 0xcc, 0x7c, 0xa3, 0x4a, 0xe1, 0x45, 0x6f,
];
#[doc = "increase.delta: 1018eb85-2d04-4995-a349-b6c83c27f287"]
pub const INCREASE_DELTA_PARAMETER_RAW_ID: [u8; 16] = [
    0x10, 0x18, 0xeb, 0x85, 0x2d, 0x04, 0x49, 0x95, 0xa3, 0x49, 0xb6, 0xc8, 0x3c, 0x27, 0xf2, 0x87,
];
#[doc = "increase.storage: e898fe88-cc61-46d2-aecc-b4fc0beb862f"]
pub const INCREASE_STORAGE_PARAMETER_RAW_ID: [u8; 16] = [
    0xe8, 0x98, 0xfe, 0x88, 0xcc, 0x61, 0x46, 0xd2, 0xae, 0xcc, 0xb4, 0xfc, 0x0b, 0xeb, 0x86, 0x2f,
];
#[doc = "seq_star: c2d5ed72-798c-4174-94f7-13378bd9bf1f"]
pub const SEQ_STAR_FUNCTION_RAW_ID: [u8; 16] = [
    0xc2, 0xd5, 0xed, 0x72, 0x79, 0x8c, 0x41, 0x74, 0x94, 0xf7, 0x13, 0x37, 0x8b, 0xd9, 0xbf, 0x1f,
];
#[doc = "seq_star.current_index: 4de502df-3f48-4541-94d8-dd68fe92bc8e"]
pub const SEQ_STAR_CURRENT_INDEX_PARAMETER_RAW_ID: [u8; 16] = [
    0x4d, 0xe5, 0x02, 0xdf, 0x3f, 0x48, 0x45, 0x41, 0x94, 0xd8, 0xdd, 0x68, 0xfe, 0x92, 0xbc, 0x8e,
];
#[doc = "seq_star.children: 5b6e9515-dbcc-411d-bee9-3d8cba5fedda"]
pub const SEQ_STAR_CHILDREN_PARAMETER_RAW_ID: [u8; 16] = [
    0x5b, 0x6e, 0x95, 0x15, 0xdb, 0xcc, 0x41, 0x1d, 0xbe, 0xe9, 0x3d, 0x8c, 0xba, 0x5f, 0xed, 0xda,
];
#[doc = "seq: 32246df6-ab5d-4f18-9221-23e28731de93"]
pub const SEQ_FUNCTION_RAW_ID: [u8; 16] = [
    0x32, 0x24, 0x6d, 0xf6, 0xab, 0x5d, 0x4f, 0x18, 0x92, 0x21, 0x23, 0xe2, 0x87, 0x31, 0xde, 0x93,
];
#[doc = "seq.children: 5b6e9515-dbcc-411d-bee9-3d8cba5fedda"]
pub const SEQ_CHILDREN_PARAMETER_RAW_ID: [u8; 16] = [
    0x5b, 0x6e, 0x95, 0x15, 0xdb, 0xcc, 0x41, 0x1d, 0xbe, 0xe9, 0x3d, 0x8c, 0xba, 0x5f, 0xed, 0xda,
];
#[doc = "parallel: a9340289-1f30-411f-9faa-0f07d54613e8"]
pub const PARALLEL_FUNCTION_RAW_ID: [u8; 16] = [
    0xa9, 0x34, 0x02, 0x89, 0x1f, 0x30, 0x41, 0x1f, 0x9f, 0xaa, 0x0f, 0x07, 0xd5, 0x46, 0x13, 0xe8,
];
#[doc = "parallel.children: 5b6e9515-dbcc-411d-bee9-3d8cba5fedda"]
pub const PARALLEL_CHILDREN_PARAMETER_RAW_ID: [u8; 16] = [
    0x5b, 0x6e, 0x95, 0x15, 0xdb, 0xcc, 0x41, 0x1d, 0xbe, 0xe9, 0x3d, 0x8c, 0xba, 0x5f, 0xed, 0xda,
];
#[doc = "set_str: c803889f-4757-4b56-908f-4b2b47041eff"]
pub const SET_STR_FUNCTION_RAW_ID: [u8; 16] = [
    0xc8, 0x03, 0x88, 0x9f, 0x47, 0x57, 0x4b, 0x56, 0x90, 0x8f, 0x4b, 0x2b, 0x47, 0x04, 0x1e, 0xff,
];
#[doc = "set_str.value: 88438955-7872-44ad-8464-d636dc5fe26f"]
pub const SET_STR_VALUE_PARAMETER_RAW_ID: [u8; 16] = [
    0x88, 0x43, 0x89, 0x55, 0x78, 0x72, 0x44, 0xad, 0x84, 0x64, 0xd6, 0x36, 0xdc, 0x5f, 0xe2, 0x6f,
];
#[doc = "set_str.variable: 8fa2f965-1eb5-40d9-baca-8facef0d31a8"]
pub const SET_STR_VARIABLE_PARAMETER_RAW_ID: [u8; 16] = [
    0x8f, 0xa2, 0xf9, 0x65, 0x1e, 0xb5, 0x40, 0xd9, 0xba, 0xca, 0x8f, 0xac, 0xef, 0x0d, 0x31, 0xa8,
];
#[doc = "regex_match: 8e3dbcc1-1a81-4cf6-a457-6e0c075456fd"]
pub const REGEX_MATCH_FUNCTION_RAW_ID: [u8; 16] = [
    0x8e, 0x3d, 0xbc, 0xc1, 0x1a, 0x81, 0x4c, 0xf6, 0xa4, 0x57, 0x6e, 0x0c, 0x07, 0x54, 0x56, 0xfd,
];
#[doc = "regex_match.value: 3267f093-8a7f-4b77-b74c-3bd2e7ad40f9"]
pub const REGEX_MATCH_VALUE_PARAMETER_RAW_ID: [u8; 16] = [
    0x32, 0x67, 0xf0, 0x93, 0x8a, 0x7f, 0x4b, 0x77, 0xb7, 0x4c, 0x3b, 0xd2, 0xe7, 0xad, 0x40, 0xf9,
];
#[doc = "regex_match.first_match: e8b71df7-2bb5-4498-8bc3-833c5bc8eadc"]
pub const REGEX_MATCH_FIRST_MATCH_PARAMETER_RAW_ID: [u8; 16] = [
    0xe8, 0xb7, 0x1d, 0xf7, 0x2b, 0xb5, 0x44, 0x98, 0x8b, 0xc3, 0x83, 0x3c, 0x5b, 0xc8, 0xea, 0xdc,
];
#[doc = "regex_match.matcher: 6702e02d-f6ba-4c5d-acab-9ade0a690afa"]
pub const REGEX_MATCH_MATCHER_PARAMETER_RAW_ID: [u8; 16] = [
    0x67, 0x02, 0xe0, 0x2d, 0xf6, 0xba, 0x4c, 0x5d, 0xac, 0xab, 0x9a, 0xde, 0x0a, 0x69, 0x0a, 0xfa,
];
#[doc = "status_identity: ef48e6d3-c735-4b5c-8f63-fc54d94dd4ee"]
pub const STATUS_IDENTITY_FUNCTION_RAW_ID: [u8; 16] = [
    0xef, 0x48, 0xe6, 0xd3, 0xc7, 0x35, 0x4b, 0x5c, 0x8f, 0x63, 0xfc, 0x54, 0xd9, 0x4d, 0xd4, 0xee,
];
#[doc = "status_identity.value: e1f174e6-ca9e-4344-84cb-7f3f22115239"]
pub const STATUS_IDENTITY_VALUE_PARAMETER_RAW_ID: [u8; 16] = [
    0xe1, 0xf1, 0x74, 0xe6, 0xca, 0x9e, 0x43, 0x44, 0x84, 0xcb, 0x7f, 0x3f, 0x22, 0x11, 0x52, 0x39,
];
#[doc = "run: 41ae5ed0-1d12-4b71-aab8-02e7efedf177"]
pub const RUN_FUNCTION_RAW_ID: [u8; 16] = [
    0x41, 0xae, 0x5e, 0xd0, 0x1d, 0x12, 0x4b, 0x71, 0xaa, 0xb8, 0x02, 0xe7, 0xef, 0xed, 0xf1, 0x77,
];
#[doc = "fail: 3abbbfb6-d00d-41eb-88bb-97874267eaf6"]
pub const FAIL_FUNCTION_RAW_ID: [u8; 16] = [
    0x3a, 0xbb, 0xbf, 0xb6, 0xd0, 0x0d, 0x41, 0xeb, 0x88, 0xbb, 0x97, 0x87, 0x42, 0x67, 0xea, 0xf6,
];
#[doc = "wait_str_set: 3180977c-25a1-458e-ab82-11f36c654518"]
pub const WAIT_STR_SET_FUNCTION_RAW_ID: [u8; 16] = [
    0x31, 0x80, 0x97, 0x7c, 0x25, 0xa1, 0x45, 0x8e, 0xab, 0x82, 0x11, 0xf3, 0x6c, 0x65, 0x45, 0x18,
];
#[doc = "wait_str_set.value: 8f190079-e519-44d3-ac36-3bfc322e87eb"]
pub const WAIT_STR_SET_VALUE_PARAMETER_RAW_ID: [u8; 16] = [
    0x8f, 0x19, 0x00, 0x79, 0xe5, 0x19, 0x44, 0xd3, 0xac, 0x36, 0x3b, 0xfc, 0x32, 0x2e, 0x87, 0xeb,
];
