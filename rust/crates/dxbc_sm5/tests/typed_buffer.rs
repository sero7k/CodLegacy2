//! A typed buffer SRV (`Buffer<uint>`, as Infinite Warfare's forward+ cluster
//! lists are) loaded with `ld`: declared as a word array, the element read
//! by index, the WGSL valid.

fn words(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// `ps_5_0`: dcl_resource_buffer (uint,uint,uint,uint) t36; dcl_output o0;
/// dcl_temps 1; ld r0.xyzw, l(5, 0, 0, 0), t36.xyzw; mov o0, r0; ret.
fn program(ret: u32) -> Vec<u32> {
    let body = vec![
        0x0400_0858, 0x0010_7000, 36, ret, // dcl_resource_buffer t36
        0x0300_0065, 0x0010_20f2, 0, // dcl_output o0.xyzw
        0x0200_0068, 1, // dcl_temps 1
        0x0a00_002d, 0x0010_00f2, 0, 0x0000_4002, 5, 0, 0, 0, 0x0010_7e46, 36, // ld
        0x0500_0036, 0x0010_20f2, 0, 0x0010_0e46, 0, // mov o0, r0
        0x0100_003e, // ret
    ];
    let mut out = vec![0x0000_0050, (body.len() + 2) as u32];
    out.extend(body);
    out
}

fn translate(ret: u32, opts: dxbc_sm5::Options) -> String {
    let bytes = words(&program(ret));
    let prog = dxbc_sm5::decode(&bytes).expect("decodes");
    let c = dxbc_sm5::Container {
        shader: &bytes,
        inputs: Vec::new(),
        outputs: vec![dxbc_sm5::SigElement { semantic: "SV_TARGET".into(), semantic_index: 0, system_value: 0, component_type: 1, register: 0, mask: 0xf, rw_mask: 0 }],
        patch_constants: Vec::new(),
    };
    let tr = dxbc_sm5::translate_program(&prog, &c, &opts).expect("translates");
    let module = naga::front::wgsl::parse_str(&tr.wgsl).map_err(|e| e.emit_to_string(&tr.wgsl)).expect("parses");
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&module).expect("valid");
    tr.wgsl
}

#[test]
fn a_typed_buffer_is_a_word_array_read_by_element() {
    let opts = dxbc_sm5::Options::for_stage(dxbc_sm5::Stage::Pixel);
    let w = translate(0x4444, opts.clone());
    assert!(w.contains("var<storage, read> t36: array<u32>"), "{w}");
    // One word an element: x is word 5, y and z are 0, w is 1.
    assert!(w.contains("t36[(bitcast<u32>(") && w.contains(" * 1u) + 0u]") && w.contains(", 0u, 0u, 1u)"), "{w}");
    // Four words an element (a float4 buffer): all four channels read.
    let mut four = opts;
    four.srv_buffer_words = vec![(36, 4)];
    let w = translate(0x5555, four);
    assert!(w.contains(" * 4u) + 3u]") && w.contains("bitcast<vec4<f32>>"), "{w}");
}
