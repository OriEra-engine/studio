//! 单文件「双击即玩」导出：把项目 index.html 依赖的引擎模块（importmap 映射）
//! 内联为 data URL，导出的 HTML 脱离 Studio 也能玩（离线规则引擎兜底）。
//! 纯逻辑模块（解析/改写/base64），零 tauri 依赖，可独立 rustc 验证。

/// 标准 base64（带 padding）。不为此引入新依赖。
pub fn base64_encode(data: &[u8]) -> String {
    const TBL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TBL[(n >> 18) as usize & 63] as char);
        out.push(TBL[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TBL[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TBL[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// importmap 的 key → MIME。glb 是模型，其余按 JS 处理。
fn mime_for_key(key: &str) -> &'static str {
    if key.ends_with(".glb") {
        "model/gltf-binary"
    } else {
        "text/javascript"
    }
}

/// GLTFLoader 内部的相对导入在 data URL 下无法解析，改写成 importmap bare specifier。
fn rewrite_vendor_relative(source: &[u8], key: &str) -> Vec<u8> {
    if key.ends_with("GLTFLoader.js") {
        let text = String::from_utf8_lossy(source).into_owned();
        let rewritten = text.replace(
            "'../utils/BufferGeometryUtils.js'",
            "\"oriera-vendor/utils/BufferGeometryUtils.js\"",
        );
        rewritten.into_bytes()
    } else {
        source.to_vec()
    }
}

/// 把 importmap 中每个映射值换成 `resolve(url)` 给出的内容之 data URL，返回新 HTML。
/// resolve 找不到依赖即报错（快照不完整时宁可失败也不产出一个黑屏 HTML）。
pub fn build_singlefile(
    html: &str,
    resolve: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<String, String> {
    const START: &str = "<script type=\"importmap\">";
    const END: &str = "</script>";
    let start = html.find(START).ok_or("index.html 里找不到 importmap 块")?;
    let end_rel = html[start..].find(END).ok_or("importmap 块未闭合")?;
    let end = start + end_rel;
    let inner = &html[start + START.len()..end];

    let map: serde_json::Value =
        serde_json::from_str(inner).map_err(|e| format!("importmap 不是合法 JSON：{e}"))?;
    let imports = map
        .get("imports")
        .and_then(|v| v.as_object())
        .ok_or("importmap 缺少 imports 对象")?;

    let mut new_imports = serde_json::Map::new();
    for (key, val) in imports {
        let url = val
            .as_str()
            .ok_or_else(|| format!("importmap[{key}] 不是字符串"))?;
        let bytes = resolve(url).ok_or_else(|| format!("导出依赖缺失：{url}（引擎快照不完整）"))?;
        let bytes = rewrite_vendor_relative(&bytes, key);
        let data_url = format!(
            "data:{};base64,{}",
            mime_for_key(key),
            base64_encode(&bytes)
        );
        new_imports.insert(key.clone(), serde_json::Value::String(data_url));
    }
    let new_inner = serde_json::to_string(&serde_json::json!({ "imports": new_imports }))
        .map_err(|e| e.to_string())?;

    let mut out = String::with_capacity(html.len() * 2);
    out.push_str(&html[..start + START.len()]);
    out.push_str(&new_inner);
    out.push_str(&html[end..]);

    // importmap 之外还可能有 fetch 加载的资产字面量（如 GLTFLoader 拉取的 GLB——
    // fetch 不走 importmap）。把所有带引号的 "/examples/..." 字符串统一内联为 data URL。
    let mut result = out;
    let mut search_from = 0usize;
    while let Some(rel_pos) = result[search_from..].find("\"/examples/") {
        let open = search_from + rel_pos + 1; // 越过开头引号
        let close = result[open..].find('"').ok_or("资产字面量未闭合")? + open;
        let url = &result[open..close];
        let bytes = resolve(url).ok_or_else(|| format!("导出依赖缺失：{url}（引擎快照不完整）"))?;
        let data_url = format!(
            "\"data:{};base64,{}\"",
            "application/octet-stream",
            base64_encode(&bytes)
        );
        result = format!(
            "{}{}{}{}",
            &result[..open - 1],
            data_url,
            &result[close + 1..],
            ""
        );
        search_from = open - 1 + data_url.len();
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    fn fixture() -> String {
        r#"<html><head>
<script type="importmap">
{ "imports": { "oriera-kernel": "/examples/x/engine/kernel.js", "oriera-vendor/RobotExpressive.glb": "/examples/x/vendor/RobotExpressive.glb" } }
</script>
<script type="module">import("oriera-kernel")</script>
</head></html>"#
            .into()
    }

    #[test]
    fn inlines_all_mappings_as_data_urls() {
        let out = build_singlefile(&fixture(), &|url| match url {
            "/examples/x/engine/kernel.js" => Some(b"export const k=1".to_vec()),
            "/examples/x/vendor/RobotExpressive.glb" => Some(vec![0x67, 0x6c, 0x54, 0x46]),
            _ => None,
        })
        .unwrap();
        assert!(out.contains("data:text/javascript;base64,"), "{out}");
        assert!(out.contains("data:model/gltf-binary;base64,"));
        // importmap 之外的 bare specifier 动态导入保持原样（import() 走 importmap 解析）
        assert!(out.contains("import(\"oriera-kernel\")"));
        // importmap 块内不再出现原始 URL（检查块内容而非全文）
        let map_start = out.find("<script type=\"importmap\">").unwrap();
        let map_end = out[map_start..].find("</script>").unwrap() + map_start;
        assert!(!out[map_start..map_end].contains("/examples/x/"));
    }

    #[test]
    fn missing_dependency_is_hard_error() {
        let err = build_singlefile(&fixture(), &|_| None).unwrap_err();
        assert!(err.contains("导出依赖缺失"));
    }

    #[test]
    fn fetch_asset_literal_inlined() {
        let html = r#"<script type="importmap">{ "imports": {} }</script><script type="module">const GLB="/examples/v/model.glb";load(GLB);</script>"#;
        let out = build_singlefile(html, &|url| {
            if url == "/examples/v/model.glb" {
                Some(vec![1, 2, 3])
            } else {
                None
            }
        })
        .unwrap();
        assert!(
            out.contains("data:application/octet-stream;base64,"),
            "{out}"
        );
        assert!(!out.contains("/examples/v/model.glb"));
    }

    #[test]
    fn gltfloader_relative_import_rewritten() {
        let src = b"import { t } from '../utils/BufferGeometryUtils.js';".to_vec();
        let out = rewrite_vendor_relative(&src, "oriera-vendor/GLTFLoader.js");
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\"oriera-vendor/utils/BufferGeometryUtils.js\""));
        assert!(!text.contains("../utils"));
        // 非 GLTFLoader 的模块不做改写
        let untouched = rewrite_vendor_relative(&src, "oriera-vendor/OrbitControls.js");
        assert!(String::from_utf8_lossy(&untouched).contains("../utils"));
    }
}
