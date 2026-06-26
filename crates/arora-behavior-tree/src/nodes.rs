use crate::{
    schema::{Expression, _RET_PARAM_ID},
    tree_node::TreeNode,
};
use arora_types::value::Value;
use std::{cell::RefCell, collections::HashMap, rc::Rc, str::FromStr};
use uuid::Uuid;

// To simulate statuses
//===============================================================
#[allow(unused)]
pub fn succeed() -> TreeNode {
    TreeNode::action_node(SUCCEED_FUNCTION_ID)
}

#[allow(unused)]
pub fn fail() -> TreeNode {
    TreeNode::action_node(FAIL_FUNCTION_ID)
}

#[allow(unused)]
pub fn run() -> TreeNode {
    TreeNode::action_node(RUN_FUNCTION_ID)
}

#[allow(unused)]
pub fn status_identity(value: Rc<RefCell<Value>>) -> TreeNode {
    TreeNode {
        function: STATUS_IDENTITY_FUNCTION_ID,
        children: None,
        parameters: HashMap::from([(STATUS_VALUE_PARAM_ID, Expression::Variable(value))]),
    }
}

// Basic data-oriented action nodes
//==============================================================
#[allow(unused)]
pub fn set_str(variable: Expression, value: Expression) -> TreeNode {
    TreeNode {
        function: Uuid::from_str("b8349b96-abc7-4a31-906c-da1ce6fa356e").unwrap(),
        children: None,
        parameters: HashMap::from([
            (
                Uuid::from_str("8fa2f965-1eb5-40d9-baca-8facef0d31a8").unwrap(),
                variable,
            ),
            (
                Uuid::from_str("88438955-7872-44ad-8464-d636dc5fe26f").unwrap(),
                value,
            ),
        ]),
    }
}

#[allow(unused)]
pub fn unset_str(variable: Expression) -> TreeNode {
    TreeNode {
        function: Uuid::from_str("7dce01ed-9818-4b7d-b45a-2e7fdece3633").unwrap(),
        children: None,
        parameters: HashMap::from([(
            Uuid::from_str("2c84bf0f-4ec2-41a4-83ee-3f92a53be79d").unwrap(),
            variable,
        )]),
    }
}

#[allow(unused)]
pub fn is_str_set(value: Expression) -> TreeNode {
    TreeNode {
        function: Uuid::from_str("20ba3f0f-309e-4cd2-adfc-aca6cc432526").unwrap(),
        children: None,
        parameters: HashMap::from([(
            Uuid::from_str("c4f1e72d-30fe-400b-a584-f08e93944026").unwrap(),
            value,
        )]),
    }
}

#[allow(unused)]
pub fn wait_str_set(value: Expression) -> TreeNode {
    TreeNode {
        function: Uuid::from_str("3180977c-25a1-458e-ab82-11f36c654518").unwrap(),
        children: None,
        parameters: HashMap::from([(
            Uuid::from_str("8f190079-e519-44d3-ac36-3bfc322e87eb").unwrap(),
            value,
        )]),
    }
}

#[allow(unused)]
pub fn regex_match(value: Expression, matcher: Expression, first_match: Expression) -> TreeNode {
    TreeNode {
        function: Uuid::from_str("8e3dbcc1-1a81-4cf6-a457-6e0c075456fd").unwrap(),
        children: None,
        parameters: HashMap::from([
            (
                Uuid::from_str("3267f093-8a7f-4b77-b74c-3bd2e7ad40f9").unwrap(),
                value,
            ),
            (
                Uuid::from_str("6702e02d-f6ba-4c5d-acab-9ade0a690afa").unwrap(),
                matcher,
            ),
            (
                Uuid::from_str("e8b71df7-2bb5-4498-8bc3-833c5bc8eadc").unwrap(),
                first_match,
            ),
        ]),
    }
}

#[allow(unused)]
pub fn store(storage: Expression, value: Expression) -> TreeNode {
    TreeNode {
        function: STORE_FUNCTION_ID,
        children: None,
        parameters: HashMap::from([
            (STORE_STORAGE_PARAM_ID, storage),
            (STORE_VALUE_PARAM_ID, value),
        ]),
    }
}

#[allow(unused)]
pub fn increase(storage: Expression, delta: Expression) -> TreeNode {
    TreeNode {
        function: INCREASE_FUNCTION_ID,
        children: None,
        parameters: HashMap::from([
            (INCREASE_STORAGE_PARAM_ID, storage),
            (INCREASE_DELTA_PARAM_ID, delta),
        ]),
    }
}

// Basic control nodes
//==============================================================
#[allow(unused)]
pub fn seq(children: Vec<TreeNode>) -> TreeNode {
    TreeNode::control_node(SEQ_FUNCTION_ID, children)
}

#[allow(unused)]
pub fn seq_star(children: Vec<TreeNode>) -> TreeNode {
    TreeNode {
        function: SEQ_STAR_FUNCTION_ID,
        children: Some(children),
        parameters: HashMap::from([(
            SEQ_STAR_CURRENT_INDEX_PARAM_ID,
            Expression::Value(Value::U16(0)),
        )]),
    }
}

#[allow(unused)]
pub fn fallback(children: Vec<TreeNode>) -> TreeNode {
    TreeNode::control_node(FALLBACK_FUNCTION_ID, children)
}

#[allow(unused)]
pub fn parallel(children: Vec<TreeNode>) -> TreeNode {
    TreeNode::control_node(PARALLEL_FUNCTION_ID, children)
}

// Other functions from other modules
//================================================================
#[allow(unused)]
pub fn cos(angle: Expression, res: Expression) -> TreeNode {
    TreeNode {
        function: COS_FUNCTION_ID,
        children: None,
        parameters: HashMap::from([(COS_ANGLE_PARAM_ID, angle), (COS_RES_PARAM_ID, res)]),
    }
}

pub const SUCCEED_FUNCTION_ID: Uuid = Uuid::from_bytes([
    0x66, 0x96, 0xF0, 0xBD, 0xE7, 0x81, 0x40, 0xCD, 0xAE, 0xB5, 0x8D, 0xC6, 0x16, 0xF8, 0x10, 0xD2,
]);
pub const FAIL_FUNCTION_ID: Uuid = Uuid::from_bytes([
    0x3a, 0xbb, 0xbf, 0xb6, 0xd0, 0x0d, 0x41, 0xeb, 0x88, 0xbb, 0x97, 0x87, 0x42, 0x67, 0xea, 0xf6,
]);
pub const RUN_FUNCTION_ID: Uuid = Uuid::from_bytes([
    0x41, 0xae, 0x5e, 0xd0, 0x1d, 0x12, 0x4b, 0x71, 0xaa, 0xb8, 0x02, 0xe7, 0xef, 0xed, 0xf1, 0x77,
]);
pub const STATUS_IDENTITY_FUNCTION_ID: Uuid = Uuid::from_bytes([
    0xef, 0x48, 0xe6, 0xd3, 0xc7, 0x35, 0x4b, 0x5c, 0x8f, 0x63, 0xfc, 0x54, 0xd9, 0x4d, 0xd4, 0xee,
]);
pub const STATUS_VALUE_PARAM_ID: Uuid = Uuid::from_bytes([
    0xe1, 0xf1, 0x74, 0xe6, 0xca, 0x9e, 0x43, 0x44, 0x84, 0xcb, 0x7f, 0x3f, 0x22, 0x11, 0x52, 0x39,
]);

pub const STORE_FUNCTION_ID: Uuid = Uuid::from_bytes([
    0xb8, 0x34, 0x9b, 0x96, 0xab, 0xc7, 0x4a, 0x31, 0x90, 0x6c, 0xda, 0x1c, 0xe6, 0xfa, 0x35, 0x6e,
]);
pub const STORE_STORAGE_PARAM_ID: Uuid = Uuid::from_bytes([
    0x23, 0x45, 0xa3, 0xa5, 0xa8, 0x0d, 0x44, 0x80, 0x99, 0x27, 0x3c, 0x65, 0xbd, 0x2b, 0x75, 0x43,
]);
pub const STORE_VALUE_PARAM_ID: Uuid = Uuid::from_bytes([
    0x0a, 0x07, 0x78, 0xcd, 0xcb, 0x7a, 0x41, 0xfc, 0x96, 0xd4, 0x51, 0x2c, 0xc8, 0x53, 0x8c, 0xe2,
]);

pub const INCREASE_FUNCTION_ID: Uuid = Uuid::from_bytes([
    0x7f, 0x6f, 0xc4, 0xa9, 0x56, 0x7c, 0x4f, 0x15, 0x87, 0xcc, 0x7c, 0xa3, 0x4a, 0xe1, 0x45, 0x6f,
]);
pub const INCREASE_STORAGE_PARAM_ID: Uuid = Uuid::from_bytes([
    0xe8, 0x98, 0xfe, 0x88, 0xcc, 0x61, 0x46, 0xd2, 0xae, 0xcc, 0xb4, 0xfc, 0x0b, 0xeb, 0x86, 0x2f,
]);
pub const INCREASE_DELTA_PARAM_ID: Uuid = Uuid::from_bytes([
    0x10, 0x18, 0xeb, 0x85, 0x2d, 0x04, 0x49, 0x95, 0xa3, 0x49, 0xb6, 0xc8, 0x3c, 0x27, 0xf2, 0x87,
]);

pub const SEQ_FUNCTION_ID: Uuid = Uuid::from_bytes([
    0x32, 0x24, 0x6d, 0xf6, 0xab, 0x5d, 0x4f, 0x18, 0x92, 0x21, 0x23, 0xe2, 0x87, 0x31, 0xde, 0x93,
]);
pub const SEQ_STAR_FUNCTION_ID: Uuid = Uuid::from_bytes([
    0xc2, 0xd5, 0xed, 0x72, 0x79, 0x8c, 0x41, 0x74, 0x94, 0xf7, 0x13, 0x37, 0x8b, 0xd9, 0xbf, 0x1f,
]);
pub const SEQ_STAR_CURRENT_INDEX_PARAM_ID: Uuid = Uuid::from_bytes([
    0x4d, 0xe5, 0x02, 0xdf, 0x3f, 0x48, 0x45, 0x41, 0x94, 0xd8, 0xdd, 0x68, 0xfe, 0x92, 0xbc, 0x8e,
]);
pub const FALLBACK_FUNCTION_ID: Uuid = Uuid::from_bytes([
    0xbf, 0xa8, 0x9a, 0x4e, 0xc3, 0x69, 0x43, 0x0e, 0xbe, 0x78, 0x0d, 0xc0, 0x73, 0x11, 0x39, 0x1c,
]);
pub const PARALLEL_FUNCTION_ID: Uuid = Uuid::from_bytes([
    0xa9, 0x34, 0x02, 0x89, 0x1f, 0x30, 0x41, 0x1f, 0x9f, 0xaa, 0x0f, 0x07, 0xd5, 0x46, 0x13, 0xe8,
]);

// Direct action nodes from test-rust-wasm (non-Status returns via _ret out-parameter)
//==============================================================
#[allow(unused)]
pub fn cos_raw(angle: Expression, result: Rc<RefCell<Value>>) -> TreeNode {
    TreeNode {
        function: TEST_RUST_WASM_COS_FUNCTION_ID,
        children: None,
        parameters: HashMap::from([
            (TEST_RUST_WASM_COS_ANGLE_PARAM_ID, angle),
            (_RET_PARAM_ID, Expression::Variable(result)),
        ]),
    }
}

#[allow(unused)]
pub fn add_raw(a: Expression, b: Expression, result: Rc<RefCell<Value>>) -> TreeNode {
    TreeNode {
        function: TEST_RUST_WASM_ADD_FUNCTION_ID,
        children: None,
        parameters: HashMap::from([
            (TEST_RUST_WASM_ADD_A_PARAM_ID, a),
            (TEST_RUST_WASM_ADD_B_PARAM_ID, b),
            (_RET_PARAM_ID, Expression::Variable(result)),
        ]),
    }
}

pub const TEST_RUST_WASM_COS_FUNCTION_ID: Uuid = Uuid::from_bytes([
    0xc1, 0x37, 0x57, 0xcb, 0x23, 0x11, 0x4c, 0x93, 0xab, 0xcc, 0xcb, 0x12, 0xd6, 0xcb, 0xb8, 0x59,
]);
pub const TEST_RUST_WASM_COS_ANGLE_PARAM_ID: Uuid = Uuid::from_bytes([
    0x6c, 0x2a, 0x15, 0x7c, 0x42, 0x35, 0x47, 0xb0, 0xbf, 0xf3, 0x1e, 0xee, 0xf3, 0xe5, 0x74, 0x7d,
]);
pub const TEST_RUST_WASM_ADD_FUNCTION_ID: Uuid = Uuid::from_bytes([
    0xe4, 0xb0, 0xa2, 0xf3, 0x6c, 0x7d, 0x4e, 0x8f, 0x9a, 0x0b, 0x1c, 0x2d, 0x3e, 0x4f, 0x5a, 0x6b,
]);
pub const TEST_RUST_WASM_ADD_A_PARAM_ID: Uuid = Uuid::from_bytes([
    0xa1, 0xb2, 0xc3, 0xd4, 0xe5, 0xf6, 0x4a, 0x8b, 0x9c, 0x0d, 0xe1, 0xf2, 0xa3, 0xb4, 0xc5, 0xd6,
]);
pub const TEST_RUST_WASM_ADD_B_PARAM_ID: Uuid = Uuid::from_bytes([
    0xb2, 0xc3, 0xd4, 0xe5, 0xf6, 0xa7, 0x4b, 0x9c, 0x8d, 0x1e, 0xf2, 0xa3, 0xb4, 0xc5, 0xd6, 0xe7,
]);
pub const COS_FUNCTION_ID: Uuid = Uuid::from_bytes([
    0x10, 0x4b, 0x97, 0x10, 0x5d, 0x43, 0x4a, 0x93, 0x94, 0x4c, 0xd6, 0x4b, 0xdd, 0xb3, 0x0e, 0xf8,
]);
pub const COS_ANGLE_PARAM_ID: Uuid = Uuid::from_bytes([
    0x27, 0x2f, 0xba, 0xfd, 0xc2, 0xa5, 0x4f, 0xfe, 0xa2, 0x94, 0x9c, 0xab, 0xe6, 0xe6, 0xc1, 0xe7,
]);
pub const COS_RES_PARAM_ID: Uuid = Uuid::from_bytes([
    0x1d, 0x10, 0x16, 0x86, 0x05, 0xd8, 0x47, 0xb4, 0x92, 0x92, 0xfd, 0xc9, 0xe5, 0xa0, 0xda, 0xeb,
]);

pub const ADD_FUNCTION_ID: Uuid = Uuid::from_bytes([
    0x65, 0xbe, 0x1f, 0xe9, 0xac, 0x2a, 0x4b, 0x6e, 0x88, 0x70, 0x68, 0xac, 0x7b, 0xde, 0x6f, 0x0a,
]);
pub const ADD_A_PARAM_ID: Uuid = Uuid::from_bytes([
    0x0b, 0x88, 0x85, 0xb0, 0xaf, 0xca, 0x43, 0x78, 0xab, 0xe6, 0x79, 0xe2, 0xff, 0x0e, 0xe7, 0x2b,
]);
pub const ADD_B_PARAM_ID: Uuid = Uuid::from_bytes([
    0xcb, 0xb2, 0x1d, 0x3d, 0x69, 0xb1, 0x48, 0x8b, 0xa3, 0xc8, 0x23, 0x6c, 0xa6, 0x82, 0x63, 0xae,
]);
pub const ADD_RES_PARAM_ID: Uuid = Uuid::from_bytes([
    0x13, 0xd7, 0xa1, 0xc2, 0x2d, 0x37, 0x4d, 0x0e, 0xb3, 0x17, 0x29, 0x24, 0x67, 0x1d, 0x22, 0x10,
]);
