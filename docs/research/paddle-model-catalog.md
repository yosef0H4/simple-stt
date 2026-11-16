# Paddle OCR recognition catalog

Research for OAR 0.10.2; upstream metadata retrieved 2026-11-05. [OAR’s versioned model guide](https://github.com/GreatV/oar-ocr/blob/v0.10.2/docs/models.md) identifies the following as CTC text recognizers. Exact artifact URLs, byte counts and SHA-256 values are recorded in [the machine-readable catalog](../../artifacts/paddle-catalog/upstream-models.json). Release hashes and sizes come from the [v0.3.0](https://api.github.com/repos/GreatV/oar-ocr/releases/tags/v0.3.0), [v0.7.0](https://api.github.com/repos/GreatV/oar-ocr/releases/tags/v0.7.0), and [v0.1.0](https://api.github.com/repos/GreatV/oar-ocr/releases/tags/v0.1.0) release API metadata. The v3 script dictionaries are downloaded from PaddleOCR at the pinned commit recorded in the JSON; their lengths and SHA-256 are locally calculated. This establishes artifact identity, not proof of runtime compatibility or recognition quality.

| Recognition ONNX | Dictionary |
| --- | --- |
| `pp-ocrv3_mobile_rec.onnx` | `ppocr_keys_v1.txt` |
| `pp-ocrv4_mobile_rec.onnx` | `ppocr_keys_v1.txt` |
| `pp-ocrv4_server_rec.onnx` | `ppocr_keys_v1.txt` |
| `pp-ocrv4_server_rec_doc.onnx` | `ppocrv4_doc_dict.txt` |
| `pp-ocrv5_mobile_rec.onnx` | `ppocrv5_dict.txt` |
| `pp-ocrv5_server_rec.onnx` | `ppocrv5_dict.txt` |
| `ch_svtrv2_rec.onnx` | `ppocr_keys_v1.txt` |
| `ch_repsvtr_rec.onnx` | `ppocr_keys_v1.txt` |
| `arabic_pp-ocrv3_mobile_rec.onnx` | `arabic_dict.txt` |
| `arabic_pp-ocrv5_mobile_rec.onnx` | `ppocrv5_arabic_dict.txt` |
| `chinese_cht_pp-ocrv3_mobile_rec.onnx` | `chinese_cht_dict.txt` |
| `cyrillic_pp-ocrv3_mobile_rec.onnx` | `cyrillic_dict.txt` |
| `cyrillic_pp-ocrv5_mobile_rec.onnx` | `ppocrv5_cyrillic_dict.txt` |
| `devanagari_pp-ocrv3_mobile_rec.onnx` | `devanagari_dict.txt` |
| `devanagari_pp-ocrv5_mobile_rec.onnx` | `ppocrv5_devanagari_dict.txt` |
| `el_pp-ocrv5_mobile_rec.onnx` | `ppocrv5_el_dict.txt` |
| `en_pp-ocrv3_mobile_rec.onnx` | `en_dict.txt` |
| `en_pp-ocrv4_mobile_rec.onnx` | `en_dict.txt` |
| `en_pp-ocrv5_mobile_rec.onnx` | `ppocrv5_en_dict.txt` |
| `eslav_pp-ocrv5_mobile_rec.onnx` | `ppocrv5_eslav_dict.txt` |
| `japan_pp-ocrv3_mobile_rec.onnx` | `japan_dict.txt` |
| `ka_pp-ocrv3_mobile_rec.onnx` | `ka_dict.txt` |
| `korean_pp-ocrv3_mobile_rec.onnx` | `korean_dict.txt` |
| `korean_pp-ocrv5_mobile_rec.onnx` | `ppocrv5_korean_dict.txt` |
| `latin_pp-ocrv3_mobile_rec.onnx` | `latin_dict.txt` |
| `latin_pp-ocrv5_mobile_rec.onnx` | `ppocrv5_latin_dict.txt` |
| `ta_pp-ocrv3_mobile_rec.onnx` | `ta_dict.txt` |
| `ta_pp-ocrv5_mobile_rec.onnx` | `ppocrv5_ta_dict.txt` |
| `te_pp-ocrv3_mobile_rec.onnx` | `te_dict.txt` |
| `te_pp-ocrv5_mobile_rec.onnx` | `ppocrv5_te_dict.txt` |
| `th_pp-ocrv5_mobile_rec.onnx` | `ppocrv5_th_dict.txt` |
| `pp-ocrv6_tiny_rec.onnx` | `ppocrv6_tiny_dict.txt` |
| `pp-ocrv6_small_rec.onnx` | `ppocrv6_dict.txt` |
| `pp-ocrv6_medium_rec.onnx` | `ppocrv6_dict.txt` |

The two SVTR entries use `ppocr_keys_v1.txt` according to PaddleOCR’s [SVTRv2](https://github.com/PaddlePaddle/PaddleOCR/blob/main/configs/rec/SVTRv2/ch_SVTRv2_rec.yml) and [RepSVTR](https://github.com/PaddlePaddle/PaddleOCR/blob/main/configs/rec/SVTRv2/ch_RepSVTR_rec.yml) configs. These are non-PP-OCR recognition architectures supported by OAR; include them only if the intended catalog includes all PaddleOCR recognizers. The PP-OCRv4 document recognizer is a text recognizer, not the document layout pipeline.

OAR’s [v6 preset](https://github.com/GreatV/oar-ocr/blob/v0.10.2/src/oarocr/ocr.rs) pairs each v6 recognizer with its same-size detector and supplies generation-specific detection defaults; it does not establish that a different detector pairing yields equivalent accuracy. Other models can use OAR’s configurable detection and recognition builders, but no inference or preprocessing compatibility was tested in this research. The source lists v4 and v5 mobile/server detectors separately. The appropriate detection/preprocessing configuration, character order and bidi handling should be validated per profile. The existing `paddle-ar-en` recommendation is a product choice; script names alone do not establish equal support for English or other languages. Paddle’s [language coverage](https://paddlepaddle.github.io/PaddleOCR/latest/en/version3.x/algorithm/PP-OCRv5/PP-OCRv5_multi_languages.html) is model-specific.

Document orientation, rectification, layout, table, formula and seal tasks are separate model types in the [OAR guide](https://github.com/GreatV/oar-ocr/blob/v0.10.2/docs/models.md), and are outside the simple text detection/recognition pipeline. No large model was downloaded, and no inference was run.

## Kannada naming discrepancy

OAR’s model guide calls `ka_pp-ocrv3_mobile_rec.onnx` Georgian, but [PaddleOCR’s recognition documentation](https://github.com/PaddlePaddle/PaddleOCR/blob/main/docs/version3.x/module_usage/text_recognition.en.md) explicitly identifies it as Kannada; its [multilingual configuration generator](https://github.com/PaddlePaddle/PaddleOCR/blob/main/configs/rec/multi_language/generate_multi_language_configs.py) also maps `ka` to Kannada. The [pinned `ka_dict.txt`](https://github.com/PaddlePaddle/PaddleOCR/blob/dab3fe35379033fdcb2d0e9572fac0b36c9a9ebf/ppocr/utils/dict/ka_dict.txt) contains Kannada characters, including ಇ (U+0C87). The model retains its upstream `ka` filenames and ID, while the catalog uses Kannada’s ISO language code `kn`. The abbreviated English, Eastern Slavic, Greek, Tamil, Telugu and Thai dictionaries were also checked for their corresponding scripts; no analogous naming mismatch was found. Dictionary character presence alone does not establish model accuracy or complete language coverage.

## Language metadata

The language tags use PaddleOCR's [pinned v3 language-group generator](https://github.com/PaddlePaddle/PaddleOCR/blob/dab3fe35379033fdcb2d0e9572fac0b36c9a9ebf/configs/rec/multi_language/generate_multi_language_configs.py), [pinned v5 model language table](https://github.com/PaddlePaddle/PaddleOCR/blob/dab3fe35379033fdcb2d0e9572fac0b36c9a9ebf/docs/version3.x/algorithm/PP-OCRv5/PP-OCRv5_multi_languages.en.md), and [v6 language list](https://github.com/PaddlePaddle/PaddleOCR/blob/dab3fe35379033fdcb2d0e9572fac0b36c9a9ebf/docs/version3.x/algorithm/PP-OCRv6/PP-OCRv6.en.md). V6 tiny omits Japanese; small and medium include it. Both simplified and traditional Chinese map to `zh`. English for v5 Latin is retained from the earlier [PaddleOCR v3.1 multilingual table](https://paddlepaddle.github.io/PaddleOCR/v3.1.0/en/version3.x/algorithm/PP-OCRv5/PP-OCRv5_multi_languages.html), although the later table omits it.

Paddle aliases are normalized to language tags: `rs_latin`/`rs_cyrillic` → `sr`, `ava` → `av`, `che` → `ce`, `mah` → `mag`, and `gom` → `kok`. Kannada uses `kn` despite the upstream model's `ka` filename. V3 `ang` is omitted because Paddle's table calls it Old English while its v5 model table says Angika; v5 uses the actual Angika code `anp`. V3 `sck` is retained as Sadri; v5's table names Santali, so it uses `sat`. The v5 Cyrillic table's “Malian” is ambiguous and is omitted rather than inventing a language code; “Moldovan” maps to Romanian `ro`. These tables document supported language groups, not independent accuracy guarantees for each language.
