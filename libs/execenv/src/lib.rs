use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use stride_agent::{AgentConfig, Tool, ToolDesc, ToolRegistry};
use tokio::sync::{mpsc, oneshot};

mod artifacts;
pub use artifacts::{ArtifactKind, ArtifactSpec, ArtifactStore};

#[derive(Clone, Copy, Debug, Default)]
pub struct ResourceUsage {
    pub memory_bytes: Option<u64>,
    pub cpu_time_millis: Option<u64>,
}

pub trait ResourceObserver: Send + Sync {
    fn resource_usage(&self, _usage: ResourceUsage) {}
}

#[derive(Default)]
pub struct NoopResourceObserver;

impl ResourceObserver for NoopResourceObserver {}

#[cfg(feature = "eryx")]
pub use eryx::VolumeMount;

#[cfg(feature = "eryx")]
mod fonts;

/// Guest path the shared font cache is mounted at. matplotlib's font manager
/// scans `/usr/share/fonts` on non-Windows/macOS targets, so fonts dropped here
/// are discovered automatically; fpdf2/Pillow can register them by path.
#[cfg(feature = "eryx")]
const FONTS_GUEST_DIR: &str = "/usr/share/fonts";

#[cfg(feature = "bashkit")]
mod bashkit_cmd;
#[cfg(feature = "bashkit")]
pub use bashkit_cmd::PythonBuiltin;

#[cfg(feature = "typst")]
pub mod typst_doc;
#[cfg(feature = "typst")]
pub use typst_doc::{CompileRequest, TypstFormat, compile as typst_compile};

#[cfg(all(feature = "bashkit", feature = "typst"))]
mod typst_cmd;
#[cfg(all(feature = "bashkit", feature = "typst"))]
pub use typst_cmd::TypstBuiltin;

#[cfg(all(feature = "eryx", feature = "typst"))]
mod typst_bridge;

// Pure-Python wheels (py3-none-any). They carry no native extensions, so they
// import on any CPython-WASI minor version and need no compilation.
const BS4_URL: &str = "https://files.pythonhosted.org/packages/88/c6/92fcd42f1ba33e1184263f25bfabf3d27c383410470f169e4b8163bf9c17/beautifulsoup4-4.15.0-py3-none-any.whl";
const SOUPSIEVE_URL: &str = "https://files.pythonhosted.org/packages/5e/f5/0c41cb68dcae6b7de4fac4188a3a9589e21fb31df21ea3a2e888db95e6c9/soupsieve-2.8.4-py3-none-any.whl";
const REQUESTS_URL: &str = "https://files.pythonhosted.org/packages/a0/f4/c67b0b3f1b9245e8d266f0f112c500d50e5b4e83cb6f3b71b6528104182a/requests-2.34.2-py3-none-any.whl";
const URLLIB3_URL: &str = "https://files.pythonhosted.org/packages/7f/3e/5db95bcf282c52709639744ca2a8b149baccf648e39c8cc87553df9eae0c/urllib3-2.7.0-py3-none-any.whl";
const CERTIFI_URL: &str = "https://files.pythonhosted.org/packages/ef/2f/c5464532e965badff2f4c4c1a3a83f5697f0d7c407ed0cda44aaa99bb451/certifi-2026.6.17-py3-none-any.whl";
const IDNA_URL: &str = "https://files.pythonhosted.org/packages/1e/5e/d4e9f1a599fb8e573b7b87160658329fbf28d19eac2718f51fc3def3aa5a/idna-3.18-py3-none-any.whl";
const MARKDOWN_URL: &str = "https://files.pythonhosted.org/packages/de/1f/77fa3081e4f66ca3576c896ae5d31c3002ac6607f9747d2e3aa49227e464/markdown-3.10.2-py3-none-any.whl";
const DATEUTIL_URL: &str = "https://files.pythonhosted.org/packages/36/7a/87837f39d0296e723bb9b62bbb257d0355c7f6128853c78955f57342a56d/python_dateutil-2.8.2-py2.py3-none-any.whl";
const SIX_URL: &str = "https://files.pythonhosted.org/packages/b7/ce/149a00dd41f10bc29e5921b496af8b574d8413afcd5e30dfa0ed46c2cc5e/six-1.17.0-py2.py3-none-any.whl";
const TYPING_EXTENSIONS_URL: &str = "https://files.pythonhosted.org/packages/18/67/36e9267722cc04a6b9f15c7f3441c2363321a3ea07da7ae0c0707beb2a9c/typing_extensions-4.15.0-py3-none-any.whl";
const CHARSET_NORMALIZER_URL: &str = "https://files.pythonhosted.org/packages/db/8f/61959034484a4a7c527811f4721e75d02d653a35afb0b6054474d8185d4c/charset_normalizer-3.4.7-py3-none-any.whl";
// Pure-Python runtime dependencies of the native packages below. pandas needs
// pytz + tzdata; matplotlib needs cycler, fonttools, packaging and pyparsing.
const PYTZ_URL: &str = "https://files.pythonhosted.org/packages/ec/dd/96da98f892250475bdf2328112d7468abdd4acc7b902b6af23f4ed958ea0/pytz-2026.2-py2.py3-none-any.whl";
const TZDATA_URL: &str = "https://files.pythonhosted.org/packages/ce/e4/dccd7f47c4b64213ac01ef921a1337ee6e30e8c6466046018326977efd95/tzdata-2026.2-py2.py3-none-any.whl";
const CYCLER_URL: &str = "https://files.pythonhosted.org/packages/e7/05/c19819d5e3d95294a6f5947fb9b9629efb316b96de511b418c53d245aae6/cycler-0.12.1-py3-none-any.whl";
const FONTTOOLS_URL: &str = "https://files.pythonhosted.org/packages/2c/47/c99d5268f354002ce80f8d029cd9d7d872969da1de8b93d32de4dc56d6f4/fonttools-4.63.0-py3-none-any.whl";
const PYPARSING_URL: &str = "https://files.pythonhosted.org/packages/10/bd/c038d7cc38edc1aa5bf91ab8068b63d4308c66c4c8bb3cbba7dfbc049f9c/pyparsing-3.3.2-py3-none-any.whl";
const PACKAGING_URL: &str = "https://files.pythonhosted.org/packages/df/b2/87e62e8c3e2f4b32e5fe99e0b86d576da1312593b39f47d8ceef365e95ed/packaging-26.2-py3-none-any.whl";

// Task-oriented pure-Python packages for autonomous agents: document I/O
// (pypdf, openpyxl + et-xmlfile, markdownify, tabulate), calendar (icalendar,
// tzlocal, humanize), HTTP (httpx + httpcore + h11 + anyio), locale formatting
// (babel) and email handling (email-validator + dnspython). All py3-none-any,
// so they need no compilation and load lazily from site-packages.
const PYPDF_URL: &str = "https://files.pythonhosted.org/packages/94/56/2967e621598987905fb8cdfadd8f8de6b5c68c9351f0523c4df8409f28f1/pypdf-6.13.3-py3-none-any.whl";
// pdfplumber + pdfminer.six give layout-aware extraction (words/lines/rects with
// bounding boxes, table detection, reading order) on top of pypdf's byte access.
// Both are pure-Python; pdfminer.six reuses charset-normalizer, pdfplumber reuses
// pillow. pdfminer.six eagerly imports `cryptography` (absent on WASI) only for
// encrypted PDFs; the OCR extractor injects a lazy stub before importing it, so
// unencrypted documents work and encrypted ones fail with a clear message.
const PDFPLUMBER_URL: &str = "https://files.pythonhosted.org/packages/a2/9a/07d658e1e7fad860f1c541ab941348125dbdab773be3a0afaf32361866c7/pdfplumber-0.11.10-py3-none-any.whl";
const PDFMINER_SIX_URL: &str = "https://files.pythonhosted.org/packages/20/8b/28c4eaec9d6b036a52cb44720408f26b1a143ca9bce76cc19e8f5de00ab4/pdfminer_six-20260107-py3-none-any.whl";
const OPENPYXL_URL: &str = "https://files.pythonhosted.org/packages/c0/da/977ded879c29cbd04de313843e76868e6e13408a94ed6b987245dc7c8506/openpyxl-3.1.5-py2.py3-none-any.whl";
const ET_XMLFILE_URL: &str = "https://files.pythonhosted.org/packages/c1/8b/5fe2cc11fee489817272089c4203e679c63b570a5aaeb18d852ae3cbba6a/et_xmlfile-2.0.0-py3-none-any.whl";
const MARKDOWNIFY_URL: &str = "https://files.pythonhosted.org/packages/43/ce/f1e3e9d959db134cedf06825fae8d5b294bd368aacdd0831a3975b7c4d55/markdownify-1.2.2-py3-none-any.whl";
const TABULATE_URL: &str = "https://files.pythonhosted.org/packages/99/55/db07de81b5c630da5cbf5c7df646580ca26dfaefa593667fc6f2fe016d2e/tabulate-0.10.0-py3-none-any.whl";
const ICALENDAR_URL: &str = "https://files.pythonhosted.org/packages/a0/57/aa44e7af1244856d92a700dca5089777a334fecd328f82d5faa5c2696e2e/icalendar-7.1.3-py3-none-any.whl";
const TZLOCAL_URL: &str = "https://files.pythonhosted.org/packages/42/28/fc144409c71569e928585f8f3c629d80d1ca3ef40175e9222f01588f98c9/tzlocal-5.4.3-py3-none-any.whl";
const HUMANIZE_URL: &str = "https://files.pythonhosted.org/packages/c5/7b/bca5613a0c3b542420cf92bd5e5fb8ebd5435ce1011a091f66bb7693285e/humanize-4.15.0-py3-none-any.whl";
const HTTPX_URL: &str = "https://files.pythonhosted.org/packages/2a/39/e50c7c3a983047577ee07d2a9e53faf5a69493943ec3f6a384bdc792deb2/httpx-0.28.1-py3-none-any.whl";
const HTTPCORE_URL: &str = "https://files.pythonhosted.org/packages/7e/f5/f66802a942d491edb555dd61e3a9961140fd64c90bce1eafd741609d334d/httpcore-1.0.9-py3-none-any.whl";
const H11_URL: &str = "https://files.pythonhosted.org/packages/04/4b/29cac41a4d98d144bf5f6d33995617b185d14b22401f75ca86f384e87ff1/h11-0.16.0-py3-none-any.whl";
const ANYIO_URL: &str = "https://files.pythonhosted.org/packages/ba/16/9826f089383c593cdfc4a6e5aca94d9e91ae1692c57af82c3b2aa5e810f7/anyio-4.14.0-py3-none-any.whl";
const BABEL_URL: &str = "https://files.pythonhosted.org/packages/77/f5/21d2de20e8b8b0408f0681956ca2c69f1320a3848ac50e6e7f39c6159675/babel-2.18.0-py3-none-any.whl";
const EMAIL_VALIDATOR_URL: &str = "https://files.pythonhosted.org/packages/de/15/545e2b6cf2e3be84bc1ed85613edd75b8aea69807a71c26f4ca6a9258e82/email_validator-2.3.0-py3-none-any.whl";
const DNSPYTHON_URL: &str = "https://files.pythonhosted.org/packages/ba/5a/18ad964b0086c6e62e2e7500f7edc89e3faa45033c71c1893d34eed2b2de/dnspython-2.8.0-py3-none-any.whl";
// Office-document authoring (python-docx, python-pptx, fpdf2). python-docx
// reuses lxml; python-pptx adds xlsxwriter and reuses lxml + pillow; fpdf2 adds
// defusedxml and reuses fonttools + pillow.
const PYTHON_DOCX_URL: &str = "https://files.pythonhosted.org/packages/d0/00/1e03a4989fa5795da308cd774f05b704ace555a70f9bf9d3be057b680bcf/python_docx-1.2.0-py3-none-any.whl";
const PYTHON_PPTX_URL: &str = "https://files.pythonhosted.org/packages/d9/4f/00be2196329ebbff56ce564aa94efb0fbc828d00de250b1980de1a34ab49/python_pptx-1.0.2-py3-none-any.whl";
const FPDF2_URL: &str = "https://files.pythonhosted.org/packages/66/0a/cf50ecffa1e3747ed9380a3adfc829259f1f86b3fdbd9e505af789003141/fpdf2-2.8.7-py3-none-any.whl";
const XLSXWRITER_URL: &str = "https://files.pythonhosted.org/packages/3a/0c/3662f4a66880196a590b202f0db82d919dd2f89e99a27fadef91c4a33d41/xlsxwriter-3.2.9-py3-none-any.whl";
const DEFUSEDXML_URL: &str = "https://files.pythonhosted.org/packages/07/6c/aa3f2f849e01cb6a001cd8554a88d4c77c5c1a31c95bdf1cf9301e6d9ef4/defusedxml-0.7.1-py2.py3-none-any.whl";

// CPython 3.14 WASI standard library, downloaded on first run instead of being
// embedded in the binary (eryx's `embedded-stdlib` feature). The archive root is
// `cpython/`, so the stdlib lands at `cpython/lib/python3.14`.
#[cfg(feature = "eryx")]
const CPYTHON_STDLIB_URL: &str =
    "https://github.com/frontiers-labs/wasi-wheels/releases/download/latest/cpython-wasi.tar.gz";

#[cfg(feature = "eryx")]
const CPYTHON_STDLIB: ArtifactSpec = ArtifactSpec {
    name: "cpython-stdlib",
    url: CPYTHON_STDLIB_URL,
    sha256: "114b09fc99ee38f3f6dcdb3b4e6743e44d362f08e2ca91bacffd9ff44b33e996",
    kind: ArtifactKind::TarGz,
};

// Native (wasm32-wasip1) packages built against eryx-runtime's exact toolchain
// (wasi-sdk-27 + CPython 3.14) and published by frontiers-labs/wasi-wheels.
const NUMPY_URL: &str =
    "https://github.com/frontiers-labs/wasi-wheels/releases/download/latest/numpy-wasi.tar.gz";
const PILLOW_URL: &str =
    "https://github.com/frontiers-labs/wasi-wheels/releases/download/latest/pillow-wasi.tar.gz";
const KIWISOLVER_URL: &str =
    "https://github.com/frontiers-labs/wasi-wheels/releases/download/latest/kiwisolver-wasi.tar.gz";
const CONTOURPY_URL: &str =
    "https://github.com/frontiers-labs/wasi-wheels/releases/download/latest/contourpy-wasi.tar.gz";
const PANDAS_URL: &str =
    "https://github.com/frontiers-labs/wasi-wheels/releases/download/latest/pandas-wasi.tar.gz";
const MATPLOTLIB_URL: &str =
    "https://github.com/frontiers-labs/wasi-wheels/releases/download/latest/matplotlib-wasi.tar.gz";
const LXML_URL: &str =
    "https://github.com/frontiers-labs/wasi-wheels/releases/download/latest/lxml-wasi.tar.gz";

// Native packages (numpy, Pillow, pandas, matplotlib, ...) are built against
// eryx-runtime's exact toolchain (wasi-sdk-27 + CPython 3.14) and published by
// frontiers-labs/wasi-wheels. Their `.so` files are baked into the preinit
// snapshot (see prepare_preinit). Earlier bkmashiro builds linked a different
// wasi-libc and failed preinit with an unresolved `__wasi_init_tp` symbol; the
// frontiers-labs builds fix that. Only numpy is imported at preinit time; the
// rest load lazily so a failure surfaces at `import` in user code rather than
// breaking the whole runtime.
const WASI_PACKAGES: &[ArtifactSpec] = &[
    ArtifactSpec {
        name: "beautifulsoup4",
        url: BS4_URL,
        sha256: "d6f88de62e1d4e38ecb1077eb9724cd0eff29d2a08ca16a401e9b9e93f117cf9",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "soupsieve",
        url: SOUPSIEVE_URL,
        sha256: "e7e6b0769c8f51ed59acab6e994b00621096cfb1c640a7509295987388fbaf65",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "requests",
        url: REQUESTS_URL,
        sha256: "2a0d60c172f83ac6ab31e4554906c0f3b3588d37b5cb939b1c061f4907e278e0",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "urllib3",
        url: URLLIB3_URL,
        sha256: "9fb4c81ebbb1ce9531cce37674bbc6f1360472bc18ca9a553ede278ef7276897",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "certifi",
        url: CERTIFI_URL,
        sha256: "2227dcbaafe0d2f59279d1762ddddc37783ed4354594f194ffc31d20f41fc3db",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "idna",
        url: IDNA_URL,
        sha256: "7f952cbe720b688055e3f87de14f5c3e5fdaa8bc3928985c4077ca689de849a2",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "markdown",
        url: MARKDOWN_URL,
        sha256: "e91464b71ae3ee7afd3017d9f358ef0baf158fd9a298db92f1d4761133824c36",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "python-dateutil",
        url: DATEUTIL_URL,
        sha256: "961d03dc3453ebbc59dbdea9e4e11c5651520a876d0f4db161e8674aae935da9",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "six",
        url: SIX_URL,
        sha256: "4721f391ed90541fddacab5acf947aa0d3dc7d27b2e1e8eda2be8970586c3274",
        kind: ArtifactKind::Zip,
    },
    // typing-extensions is a runtime dependency of beautifulsoup4 4.15.
    ArtifactSpec {
        name: "typing-extensions",
        url: TYPING_EXTENSIONS_URL,
        sha256: "f0fa19c6845758ab08074a0cfa8b7aecb71c999ca73d62883bc25cc018c4e548",
        kind: ArtifactKind::Zip,
    },
    // charset-normalizer is requests' optional encoding detector; without it
    // requests imports but warns on every run.
    ArtifactSpec {
        name: "charset-normalizer",
        url: CHARSET_NORMALIZER_URL,
        sha256: "3dce51d0f5e7951f8bb4900c257dad282f49190fdbebecd4ba99bcc41fef404d",
        kind: ArtifactKind::Zip,
    },
    // pandas runtime deps.
    ArtifactSpec {
        name: "pytz",
        url: PYTZ_URL,
        sha256: "04156e608bee23d3792fd45c94ae47fae1036688e75032eea2e3bf0323d1f126",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "tzdata",
        url: TZDATA_URL,
        sha256: "bbe9af844f658da81a5f95019480da3a89415801f6cc966806612cc7169bffe7",
        kind: ArtifactKind::Zip,
    },
    // matplotlib runtime deps.
    ArtifactSpec {
        name: "cycler",
        url: CYCLER_URL,
        sha256: "85cef7cff222d8644161529808465972e51340599459b8ac3ccbac5a854e0d30",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "fonttools",
        url: FONTTOOLS_URL,
        sha256: "445af2eab030a16b9171ea8bdda7ebf7d96bda2df88ee182a464252f6e05e20d",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "pyparsing",
        url: PYPARSING_URL,
        sha256: "850ba148bd908d7e2411587e247a1e4f0327839c40e2e5e6d05a007ecc69911d",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "packaging",
        url: PACKAGING_URL,
        sha256: "5fc45236b9446107ff2415ce77c807cee2862cb6fac22b8a73826d0693b0980e",
        kind: ArtifactKind::Zip,
    },
    // Task-oriented pure-Python packages.
    ArtifactSpec {
        name: "pypdf",
        url: PYPDF_URL,
        sha256: "c6e3f86afb625791510b02ad5480e94b63970bb957df75d44657c282ecc52224",
        kind: ArtifactKind::Zip,
    },
    // pdfminer.six must precede pdfplumber (its dependency); both are pure-Python.
    ArtifactSpec {
        name: "pdfminer.six",
        url: PDFMINER_SIX_URL,
        sha256: "366585ba97e80dffa8f00cebe303d2f381884d8637af4ce422f1df3ef38111a9",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "pdfplumber",
        url: PDFPLUMBER_URL,
        sha256: "7741ea81bf165b474b153e6789d10d18e06b6ddcf3ec84289c3ef2fed6802580",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "openpyxl",
        url: OPENPYXL_URL,
        sha256: "5282c12b107bffeef825f4617dc029afaf41d0ea60823bbb665ef3079dc79de2",
        kind: ArtifactKind::Zip,
    },
    // et-xmlfile is openpyxl's XML streaming writer dependency.
    ArtifactSpec {
        name: "et-xmlfile",
        url: ET_XMLFILE_URL,
        sha256: "7a91720bc756843502c3b7504c77b8fe44217c85c537d85037f0f536151b2caa",
        kind: ArtifactKind::Zip,
    },
    // markdownify turns fetched HTML into Markdown; it reuses beautifulsoup4.
    ArtifactSpec {
        name: "markdownify",
        url: MARKDOWNIFY_URL,
        sha256: "3f02d3cc52714084d6e589f70397b6fc9f2f3a8531481bf35e8cc39f975e186a",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "tabulate",
        url: TABULATE_URL,
        sha256: "f0b0622e567335c8fabaaa659f1b33bcb6ddfe2e496071b743aa113f8774f2d3",
        kind: ArtifactKind::Zip,
    },
    // icalendar parses/builds .ics; it relies on python-dateutil and tzdata.
    ArtifactSpec {
        name: "icalendar",
        url: ICALENDAR_URL,
        sha256: "690f30aa50a76cbf854db5ad52654705db9c5cd0e1b152222f5d4b7854b60667",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "tzlocal",
        url: TZLOCAL_URL,
        sha256: "24ce97bb58e2a973f7640ec2553ab4e6f6d5a0d0d1aa9dc43bca21d89e1feb82",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "humanize",
        url: HUMANIZE_URL,
        sha256: "b1186eb9f5a9749cd9cb8565aee77919dd7c8d076161cf44d70e59e3301e1769",
        kind: ArtifactKind::Zip,
    },
    // httpx is a modern HTTP client; httpcore, h11 and anyio are its transport
    // stack. It reuses the existing certifi and idna wheels.
    ArtifactSpec {
        name: "httpx",
        url: HTTPX_URL,
        sha256: "d909fcccc110f8c7faf814ca82a9a4d816bc5a6dbfea25d6591d6985b8ba59ad",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "httpcore",
        url: HTTPCORE_URL,
        sha256: "2d400746a40668fc9dec9810239072b40b4484b640a8c38fd654a024c7a1bf55",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "h11",
        url: H11_URL,
        sha256: "63cf8bbe7522de3bf65932fda1d9c2772064ffb3dae62d55932da54b31cb6c86",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "anyio",
        url: ANYIO_URL,
        sha256: "dd9b7a2a9799ed6552fde617b2c5df02b7fdd7d88392fc48101e51bae46164d9",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "babel",
        url: BABEL_URL,
        sha256: "e2b422b277c2b9a9630c1d7903c2a00d0830c409c59ac8cae9081c92f1aeba35",
        kind: ArtifactKind::Zip,
    },
    // email-validator checks address syntax; dnspython is its resolver backend.
    ArtifactSpec {
        name: "email-validator",
        url: EMAIL_VALIDATOR_URL,
        sha256: "80f13f623413e6b197ae73bb10bf4eb0908faf509ad8362c5edeb0be7fd450b4",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "dnspython",
        url: DNSPYTHON_URL,
        sha256: "01d9bbc4a2d76bf0db7c1f729812ded6d912bd318d3b1cf81d30c0f845dbf3af",
        kind: ArtifactKind::Zip,
    },
    // Office-document authoring.
    ArtifactSpec {
        name: "python-docx",
        url: PYTHON_DOCX_URL,
        sha256: "3fd478f3250fbbbfd3b94fe1e985955737c145627498896a8a6bf81f4baf66c7",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "python-pptx",
        url: PYTHON_PPTX_URL,
        sha256: "160838e0b8565a8b1f67947675886e9fea18aa5e795db7ae531606d68e785cba",
        kind: ArtifactKind::Zip,
    },
    // xlsxwriter is python-pptx's chart-data writer dependency.
    ArtifactSpec {
        name: "xlsxwriter",
        url: XLSXWRITER_URL,
        sha256: "9a5db42bc5dff014806c58a20b9eae7322a134abb6fce3c92c181bfb275ec5b3",
        kind: ArtifactKind::Zip,
    },
    ArtifactSpec {
        name: "fpdf2",
        url: FPDF2_URL,
        sha256: "d391fc508a3ce02fc43a577c830cda4fe6f37646f2d143d489839940932fbc19",
        kind: ArtifactKind::Zip,
    },
    // defusedxml is fpdf2's hardened XML parser dependency.
    ArtifactSpec {
        name: "defusedxml",
        url: DEFUSEDXML_URL,
        sha256: "a352e7e428770286cc899e2542b6cdaedb2b4953ff269a210103ec58f6198a61",
        kind: ArtifactKind::Zip,
    },
    // Native packages (wasm32-wasip1). Their `.so` files are baked into the
    // preinit snapshot. numpy is imported at preinit to warm the snapshot; the
    // others load lazily.
    ArtifactSpec {
        name: "numpy",
        url: NUMPY_URL,
        sha256: "72d0386b9c501753c60f30d4fa90c3bcc3bce2e967214447d89a8e8f0587307b",
        kind: ArtifactKind::TarGz,
    },
    ArtifactSpec {
        name: "pillow",
        url: PILLOW_URL,
        sha256: "f88a7aa1af8abb1d3b9dc16c879522f3af7bc3acb6a64b369834df992271cf47",
        kind: ArtifactKind::TarGz,
    },
    ArtifactSpec {
        name: "kiwisolver",
        url: KIWISOLVER_URL,
        sha256: "8c69b069ffb5c79c5ee534e08cd225d049f0ab083298424880859618ba455c0a",
        kind: ArtifactKind::TarGz,
    },
    ArtifactSpec {
        name: "contourpy",
        url: CONTOURPY_URL,
        sha256: "b8d828ba0fbcd70f3c263d0e12614f32a25c89de9482ab659446896e31fe15df",
        kind: ArtifactKind::TarGz,
    },
    ArtifactSpec {
        name: "pandas",
        url: PANDAS_URL,
        sha256: "6aaf491a4df1146d67bba3297f5287beb103fdbbc975c40d1a916c3226805d4e",
        kind: ArtifactKind::TarGz,
    },
    ArtifactSpec {
        name: "matplotlib",
        url: MATPLOTLIB_URL,
        sha256: "f46f78fd9b3302d0f711f96d8da74da26e9a2b5bb8b9e205ece40c15c65dabd6",
        kind: ArtifactKind::TarGz,
    },
    // lxml is a native libxml2/libxslt-backed XML/HTML parser. It loads lazily.
    ArtifactSpec {
        name: "lxml",
        url: LXML_URL,
        sha256: "cfe08cc81e88c7e89155797c98cf307355ce216b2e4ce9917e999b0ec5f6d1d8",
        kind: ArtifactKind::TarGz,
    },
];

/// Import name to show next to a distribution whose import differs from its
/// package name, so the tool prompt tells the model what to actually `import`.
fn import_alias(name: &str) -> Option<&'static str> {
    match name {
        "beautifulsoup4" => Some("bs4"),
        "pillow" => Some("PIL"),
        "python-dateutil" => Some("dateutil"),
        "pdfminer.six" => Some("pdfminer"),
        "python-docx" => Some("docx"),
        "python-pptx" => Some("pptx"),
        "fpdf2" => Some("fpdf"),
        "dnspython" => Some("dns"),
        _ => None,
    }
}

/// Comma-separated list of every installed package for the tool prompt, built
/// from `WASI_PACKAGES` so it never drifts as packages are added.
fn installed_packages_list() -> String {
    WASI_PACKAGES
        .iter()
        .map(|pkg| match import_alias(pkg.name) {
            Some(alias) => format!("{} (import {alias})", pkg.name),
            None => pkg.name.to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(feature = "eryx")]
fn preinit_imports() -> Vec<&'static str> {
    WASI_PACKAGES
        .iter()
        .filter(|pkg| pkg.name == "numpy")
        .map(|_| "numpy")
        .collect()
}

#[cfg(feature = "eryx")]
const ERYX_RUNTIME_CACHE_VERSION: &str = "3";

#[derive(Clone, Debug)]
pub enum BackendKind {
    Mock,
    Eryx,
}

#[derive(Clone, Debug)]
pub enum NetworkAccess {
    Blocked,
    Allowed,
}

#[derive(Clone, Debug)]
pub struct ExecutionLimits {
    pub max_runtime: Duration,
    pub max_memory_bytes: Option<u64>,
    pub max_cpu_fuel: Option<u64>,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_runtime: Duration::from_secs(30),
            max_memory_bytes: Some(128 * 1024 * 1024),
            max_cpu_fuel: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PythonToolConfig {
    pub cache_dir: PathBuf,
    pub backend: BackendKind,
    pub threads: usize,
    pub preinit: bool,
    pub limits: ExecutionLimits,
    pub network: NetworkAccess,
}

impl Default for PythonToolConfig {
    fn default() -> Self {
        Self {
            cache_dir: std::env::temp_dir().join("stride-execenv"),
            backend: BackendKind::Mock,
            threads: 1,
            preinit: true,
            limits: ExecutionLimits::default(),
            network: NetworkAccess::Blocked,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionOutput {
    pub stdout: String,
    pub stderr: String,
}

/// A tool advertised to Python scripts as `tools.<name>(...)`. Built from a
/// registered agent tool's definition.
#[derive(Clone, Debug)]
pub struct PythonToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool arguments (the `parameters` object).
    pub parameters: Value,
}

/// A tool invocation requested by a running Python script. The sandbox runs on a
/// worker thread; the call is forwarded to the agent thread, which executes the
/// tool against its registry and sends the JSON result back through `reply`.
pub struct HostToolCall {
    pub name: String,
    pub args: Value,
    pub reply: oneshot::Sender<Value>,
}

#[async_trait]
pub trait ExecutionService: Send + Sync {
    async fn execute_python(&self, script: &str) -> anyhow::Result<ExecutionOutput>;

    /// Execute a script with agent tools exposed to it. `tools` are advertised
    /// as callables and each invocation is sent over `calls` for the host to
    /// run. The default implementation ignores the tools.
    async fn execute_python_with_tools(
        &self,
        script: &str,
        _tools: &[PythonToolSpec],
        _calls: mpsc::UnboundedSender<HostToolCall>,
    ) -> anyhow::Result<ExecutionOutput> {
        self.execute_python(script).await
    }
}

#[async_trait]
pub trait FileSystemBackend: Send + Sync {
    async fn before_execute(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn after_execute(&self) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(feature = "eryx")]
    fn volumes(&self) -> Vec<VolumeMount> {
        Vec::new()
    }
}

pub struct DirectOsFileSystem {
    #[cfg_attr(not(feature = "eryx"), allow(dead_code))]
    host_dir: PathBuf,
    guest_dir: String,
    read_only: bool,
}

impl DirectOsFileSystem {
    pub fn new(host_dir: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&host_dir)?;
        Ok(Self {
            host_dir,
            guest_dir: "/~workspace".to_string(),
            read_only: false,
        })
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn guest_dir(mut self, guest_dir: impl Into<String>) -> Self {
        self.guest_dir = guest_dir.into();
        self
    }
}

#[async_trait]
impl FileSystemBackend for DirectOsFileSystem {
    #[cfg(feature = "eryx")]
    fn volumes(&self) -> Vec<VolumeMount> {
        let mount = if self.read_only {
            VolumeMount::read_only(&self.host_dir, &self.guest_dir)
        } else {
            VolumeMount::new(&self.host_dir, &self.guest_dir)
        };
        vec![mount]
    }
}

#[derive(Default)]
pub struct MockExecutionService;

#[async_trait]
impl ExecutionService for MockExecutionService {
    async fn execute_python(&self, _script: &str) -> anyhow::Result<ExecutionOutput> {
        Ok(ExecutionOutput {
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

pub struct PythonTool {
    service: Arc<dyn ExecutionService>,
    registry: Option<Arc<ToolRegistry>>,
    specs: Vec<PythonToolSpec>,
}

impl PythonTool {
    pub async fn new(
        config: PythonToolConfig,
        fs: Arc<dyn FileSystemBackend>,
    ) -> anyhow::Result<Self> {
        let service: Arc<dyn ExecutionService> = match config.backend {
            BackendKind::Mock => Arc::new(MockExecutionService),
            BackendKind::Eryx => make_eryx_service(config, fs).await?,
        };
        Ok(Self {
            service,
            registry: None,
            specs: Vec::new(),
        })
    }

    pub fn from_service(service: Arc<dyn ExecutionService>) -> Self {
        Self {
            service,
            registry: None,
            specs: Vec::new(),
        }
    }

    /// The underlying interpreter, shareable with other callers (e.g. a shell
    /// `python` command) so they reuse the same runtime and workspace.
    pub fn service(&self) -> Arc<dyn ExecutionService> {
        self.service.clone()
    }

    /// Expose the agent's auto-approved tools to executed scripts under the
    /// `tools` package. The registry should not contain this Python tool itself.
    pub fn with_tools(mut self, registry: ToolRegistry) -> Self {
        self.specs = registry.auto_approved().iter().map(tool_spec).collect();
        self.specs.sort_by(|a, b| a.name.cmp(&b.name));
        self.registry = Some(Arc::new(registry));
        self
    }
}

fn tool_spec(tool: &Arc<dyn Tool>) -> PythonToolSpec {
    let definition = tool.definition();
    let parameters = definition
        .function
        .parameters
        .as_ref()
        .and_then(|params| serde_json::to_value(params).ok())
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
    PythonToolSpec {
        name: definition.function.name,
        description: definition.function.description,
        parameters,
    }
}

/// Run a single tool call from a script against the registry, refusing tools
/// that would otherwise need interactive approval.
async fn dispatch_tool_call(
    registry: &ToolRegistry,
    config: Arc<AgentConfig>,
    name: String,
    args: Value,
) -> Value {
    match registry.get(&name) {
        Some(_) if registry.needs_approval(&name, &args) => {
            json!({ "error": format!("tool '{name}' requires approval and cannot be called from python") })
        }
        Some(tool) => tool.execute(config, args).await,
        None => json!({ "error": format!("unknown tool: {name}") }),
    }
}

/// Lines describing the `tools` package for the model, one per exposed tool.
fn tools_catalog(specs: &[PythonToolSpec]) -> String {
    specs
        .iter()
        .map(|spec| {
            let params = spec
                .parameters
                .get("properties")
                .and_then(Value::as_object)
                .map(|props| props.keys().cloned().collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            let summary = spec.description.lines().next().unwrap_or_default();
            format!("- await tools.{}({params}): {summary}", spec.name)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(ToolDesc)]
struct PythonParams {
    /// Python script to execute. The sandbox preinstalls many packages; the tool
    /// description lists them. Network access is required to reach remote hosts.
    script: String,
}

#[async_trait(?Send)]
impl Tool for PythonTool {
    fn name(&self) -> &str {
        "python"
    }

    fn readable_name(&self) -> &str {
        "Python"
    }

    fn definition(&self) -> LlmTool {
        let mut description = format!(
            "Execute a Python script in a sandbox and return stdout and stderr. \
            The writable workspace is mounted at /~workspace; write outputs there. \
            /tmp is writable scratch. matplotlib uses the Agg backend. \
            Open-source fonts (DejaVu, Roboto, Open Sans, Lato, Montserrat and \
            the Noto Sans/Serif/Mono families) are mounted read-only under \
            /usr/share/fonts; matplotlib picks them up automatically. For \
            fpdf2 or Pillow register one by path, e.g. \
            /usr/share/fonts/dejavu/dejavu-fonts-ttf-2.37/ttf/DejaVuSans.ttf \
            (list /usr/share/fonts to find others). \
            Installed packages: {}.",
            installed_packages_list()
        );
        if !self.specs.is_empty() {
            description.push_str(
                "\n\nThe agent's tools are available in the `tools` package \
                (also a global). Call them as awaitable functions with keyword \
                arguments matching the tool schema; each returns the tool's JSON \
                result as a Python object. Top-level await is supported.\n",
            );
            description.push_str(&tools_catalog(&self.specs));
        }
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description,
                name: self.name().to_string(),
                parameters: Some(PythonParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, config: Arc<AgentConfig>, args: Value) -> Value {
        let params = match PythonParams::decode(args) {
            Ok(params) => params,
            Err(err) => return json!({ "error": err }),
        };

        let result = match self.registry.clone() {
            Some(registry) if !self.specs.is_empty() => {
                self.execute_with_tools(config, registry, &params.script)
                    .await
            }
            _ => self.service.execute_python(&params.script).await,
        };

        match result {
            Ok(output) => json!({
                "success": true,
                "stdout": output.stdout,
                "stderr": output.stderr,
            }),
            Err(err) => json!({
                "success": false,
                "stdout": "",
                "stderr": "",
                "error": format!("{err:#}"),
            }),
        }
    }
}

impl PythonTool {
    /// Run a script with the registry's tools exposed, servicing each tool call
    /// on this (agent) thread while the sandbox runs on its worker thread.
    async fn execute_with_tools(
        &self,
        config: Arc<AgentConfig>,
        registry: Arc<ToolRegistry>,
        script: &str,
    ) -> anyhow::Result<ExecutionOutput> {
        let (calls_tx, mut calls_rx) = mpsc::unbounded_channel::<HostToolCall>();
        let execution = self
            .service
            .execute_python_with_tools(script, &self.specs, calls_tx);
        tokio::pin!(execution);

        loop {
            tokio::select! {
                output = &mut execution => return output,
                Some(call) = calls_rx.recv() => {
                    let result =
                        dispatch_tool_call(&registry, config.clone(), call.name, call.args).await;
                    let _ = call.reply.send(result);
                }
            }
        }
    }
}

pub async fn ensure_wasi_dependencies(cache_dir: &Path) -> anyhow::Result<WasiDependencies> {
    tokio::fs::create_dir_all(cache_dir).await?;
    let deps_dir = cache_dir.join("deps");
    let site_packages = deps_dir.join("site-packages");
    tokio::fs::create_dir_all(&site_packages).await?;
    let store = ArtifactStore::new(&deps_dir);

    for pkg in WASI_PACKAGES {
        store.ensure_into(pkg, &site_packages).await?;
    }

    Ok(WasiDependencies { site_packages })
}

/// Downloads and extracts the CPython WASI stdlib into the cache dir on first
/// run, returning the path to the stdlib directory. Re-downloads if the source
/// URL changes. Replaces eryx's embedded stdlib.
#[cfg(feature = "eryx")]
async fn ensure_cpython_stdlib(cache_dir: &Path) -> anyhow::Result<PathBuf> {
    let runtime_dir = cache_dir.join("runtime");
    let stdlib = runtime_dir.join("cpython").join("lib").join("python3.14");
    ArtifactStore::new(&runtime_dir)
        .ensure_into(&CPYTHON_STDLIB, &runtime_dir)
        .await?;
    anyhow::ensure!(
        stdlib.join("encodings").exists(),
        "CPython stdlib artifact does not contain encodings"
    );
    Ok(stdlib)
}

#[derive(Clone, Debug)]
pub struct WasiDependencies {
    pub site_packages: PathBuf,
}

#[cfg(feature = "eryx")]
mod eryx_backend {
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{Arc, Mutex, OnceLock, mpsc},
    };

    use anyhow::Context;
    use async_trait::async_trait;
    use serde_json::json;
    use tokio::sync::{OnceCell, oneshot};

    use crate::{
        ERYX_RUNTIME_CACHE_VERSION, ExecutionLimits, ExecutionOutput, ExecutionService,
        FileSystemBackend, HostToolCall, NetworkAccess, PythonToolConfig, PythonToolSpec,
        ensure_wasi_dependencies,
    };

    pub struct EryxExecutionService {
        config: PythonToolConfig,
        fs: Arc<dyn FileSystemBackend>,
        runtime: Arc<OnceCell<PreinitRuntime>>,
        hub: Arc<ExecutionHub>,
    }

    impl EryxExecutionService {
        pub async fn new(
            config: PythonToolConfig,
            fs: Arc<dyn FileSystemBackend>,
        ) -> anyhow::Result<Self> {
            let runtime = runtime_cell(&config);
            let hub = execution_hub(&config);
            Ok(Self {
                config,
                fs,
                runtime,
                hub,
            })
        }
    }

    pub(super) async fn prepare_runtime(config: PythonToolConfig) -> anyhow::Result<()> {
        let runtime = runtime_cell(&config);
        let hub = execution_hub(&config);
        runtime
            .get_or_try_init(|| hub.prepare(config.clone()))
            .await
            .map(|_| ())
    }

    impl EryxExecutionService {
        async fn prepared_request(&self, script: &str) -> anyhow::Result<ExecutionRequest> {
            let runtime = self
                .runtime
                .get_or_try_init(|| self.hub.prepare(self.config.clone()))
                .await?;
            Ok(ExecutionRequest {
                runtime: Arc::new(runtime.clone()),
                script: script.to_string(),
                limits: self.config.limits.clone(),
                volumes: self.fs.volumes(),
                network: self.config.network.clone(),
                package_cache: self.config.cache_dir.join("typst-packages"),
                fonts_dir: runtime.fonts_dir.clone(),
            })
        }
    }

    fn join_results(
        result: anyhow::Result<ExecutionOutput>,
        after: anyhow::Result<()>,
    ) -> anyhow::Result<ExecutionOutput> {
        match (result, after) {
            (Ok(output), Ok(())) => Ok(output),
            (Err(err), _) => Err(err),
            (Ok(_), Err(err)) => Err(err).context("sync eryx filesystem after execute"),
        }
    }

    #[async_trait]
    impl ExecutionService for EryxExecutionService {
        async fn execute_python(&self, script: &str) -> anyhow::Result<ExecutionOutput> {
            let request = self.prepared_request(script).await?;
            self.fs.before_execute().await?;
            let result = self.hub.execute(request).await;
            let after = self.fs.after_execute().await;
            join_results(result, after)
        }

        async fn execute_python_with_tools(
            &self,
            script: &str,
            tools: &[PythonToolSpec],
            calls: tokio::sync::mpsc::UnboundedSender<HostToolCall>,
        ) -> anyhow::Result<ExecutionOutput> {
            let request = self.prepared_request(script).await?;
            self.fs.before_execute().await?;
            let result = self
                .hub
                .execute_with_tools(request, tools.to_vec(), calls)
                .await;
            let after = self.fs.after_execute().await;
            join_results(result, after)
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    struct RuntimeKey {
        cache_dir: PathBuf,
        preinit: bool,
    }

    fn runtime_cell(config: &PythonToolConfig) -> Arc<OnceCell<PreinitRuntime>> {
        let key = RuntimeKey {
            cache_dir: config.cache_dir.clone(),
            preinit: config.preinit,
        };
        static RUNTIMES: OnceLock<Mutex<HashMap<RuntimeKey, Arc<OnceCell<PreinitRuntime>>>>> =
            OnceLock::new();
        let runtimes = RUNTIMES.get_or_init(|| Mutex::new(HashMap::new()));
        let mut runtimes = runtimes.lock().expect("execenv runtime registry poisoned");
        runtimes
            .entry(key)
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone()
    }

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    struct HubKey {
        cache_dir: PathBuf,
        preinit: bool,
        threads: usize,
    }

    fn execution_hub(config: &PythonToolConfig) -> Arc<ExecutionHub> {
        let threads = config.threads.max(1);
        let key = HubKey {
            cache_dir: config.cache_dir.clone(),
            preinit: config.preinit,
            threads,
        };
        static HUBS: OnceLock<Mutex<HashMap<HubKey, Arc<ExecutionHub>>>> = OnceLock::new();
        let hubs = HUBS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut hubs = hubs.lock().expect("execenv hub registry poisoned");
        hubs.entry(key)
            .or_insert_with(|| Arc::new(ExecutionHub::new(threads)))
            .clone()
    }

    async fn build_runtime(config: PythonToolConfig) -> anyhow::Result<PreinitRuntime> {
        // The shared font cache is downloaded once and mounted into every
        // execution; a download failure is non-fatal (the sandbox still runs).
        let fonts_dir = crate::fonts::ensure_fonts(&config.cache_dir).await.ok();
        if config.preinit {
            let deps = ensure_wasi_dependencies(&config.cache_dir).await?;
            let imports = crate::preinit_imports();
            prepare_preinit(
                &config.cache_dir,
                Some(&deps.site_packages),
                &imports,
                fonts_dir,
            )
            .await
        } else {
            prepare_preinit(&config.cache_dir, None, &[], fonts_dir).await
        }
    }

    #[derive(Clone)]
    struct PreinitRuntime {
        runtime: PathBuf,
        stdlib: PathBuf,
        site_packages: Option<PathBuf>,
        fonts_dir: Option<PathBuf>,
        executor: Arc<eryx::PythonExecutor>,
    }

    async fn prepare_preinit(
        cache_dir: &std::path::Path,
        site_packages: Option<&std::path::Path>,
        imports: &[&str],
        fonts_dir: Option<PathBuf>,
    ) -> anyhow::Result<PreinitRuntime> {
        let preinit_dir = cache_dir.join("preinit");
        tokio::fs::create_dir_all(&preinit_dir).await?;
        let runtime = preinit_dir.join(if imports.is_empty() {
            "python.cwasm"
        } else {
            "python-numpy.cwasm"
        });
        let marker = runtime.with_extension("version");
        let stdlib = crate::ensure_cpython_stdlib(cache_dir).await?;

        if runtime.exists() && cache_marker_matches(&marker).await? {
            return preinit_runtime(
                runtime,
                stdlib,
                site_packages.map(std::path::Path::to_path_buf),
                fonts_dir,
            );
        }

        let extensions = if let Some(site_packages) = site_packages {
            let package = eryx::ExtractedPackage::from_path(site_packages)?;
            package
                .native_extensions
                .iter()
                .map(|ext| {
                    eryx::preinit::NativeExtension::new(
                        format!("/site-packages/{}", ext.relative_path),
                        ext.bytes.clone(),
                    )
                })
                .collect()
        } else {
            Vec::new()
        };

        // This step boots CPython in WASM, imports the preinit packages and AOT
        // compiles the snapshot to native code. It is silent and CPU-bound, so
        // on a cold cache it can run for a minute and look like a hang; log
        // around it so startup progress is visible.
        tracing::info!(
            runtime = %runtime.display(),
            "building Python preinit snapshot (first run, compiles to native code; can take a minute)"
        );
        let component =
            eryx::preinit::pre_initialize(&stdlib, site_packages, imports, &extensions).await?;
        let precompiled = eryx::PythonExecutor::precompile(&component)?;
        let tmp = runtime.with_extension("cwasm.tmp");
        tokio::fs::write(&tmp, precompiled).await?;
        tokio::fs::rename(tmp, &runtime).await?;
        tokio::fs::write(&marker, ERYX_RUNTIME_CACHE_VERSION).await?;
        tracing::info!(runtime = %runtime.display(), "Python preinit snapshot ready");

        preinit_runtime(
            runtime,
            stdlib,
            site_packages.map(std::path::Path::to_path_buf),
            fonts_dir,
        )
    }

    async fn cache_marker_matches(path: &std::path::Path) -> anyhow::Result<bool> {
        match tokio::fs::read_to_string(path).await {
            Ok(version) => Ok(version.trim() == ERYX_RUNTIME_CACHE_VERSION),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err).context("read eryx runtime cache marker"),
        }
    }

    fn preinit_runtime(
        runtime: PathBuf,
        stdlib: PathBuf,
        site_packages: Option<PathBuf>,
        fonts_dir: Option<PathBuf>,
    ) -> anyhow::Result<PreinitRuntime> {
        let mut executor = unsafe {
            eryx::PythonExecutor::from_precompiled_file(&runtime)
                .context("load precompiled eryx runtime")?
        }
        .with_python_stdlib(&stdlib);
        if let Some(site_packages) = site_packages.as_ref() {
            executor = executor.with_site_packages(site_packages);
        }

        Ok(PreinitRuntime {
            runtime,
            stdlib,
            site_packages,
            fonts_dir,
            executor: Arc::new(executor),
        })
    }

    struct ExecutionHub {
        tx: mpsc::Sender<ExecutionJob>,
    }

    impl ExecutionHub {
        fn new(threads: usize) -> Self {
            let (tx, rx) = mpsc::channel();
            let rx = Arc::new(Mutex::new(rx));
            for idx in 0..threads {
                let rx = rx.clone();
                std::thread::Builder::new()
                    .name(format!("stride-eryx-{idx}"))
                    .spawn(move || worker_loop(rx))
                    .expect("eryx worker thread");
            }
            Self { tx }
        }

        async fn execute(&self, request: ExecutionRequest) -> anyhow::Result<ExecutionOutput> {
            let (tx, rx) = oneshot::channel();
            self.tx
                .send(ExecutionJob::Execute { request, tx })
                .map_err(|_| anyhow::anyhow!("eryx execution queue stopped"))?;
            rx.await.context("eryx worker stopped")?
        }

        async fn execute_with_tools(
            &self,
            request: ExecutionRequest,
            tools: Vec<PythonToolSpec>,
            calls: tokio::sync::mpsc::UnboundedSender<HostToolCall>,
        ) -> anyhow::Result<ExecutionOutput> {
            let (tx, rx) = oneshot::channel();
            self.tx
                .send(ExecutionJob::ExecuteWithTools {
                    request,
                    tools,
                    calls,
                    tx,
                })
                .map_err(|_| anyhow::anyhow!("eryx execution queue stopped"))?;
            rx.await.context("eryx worker stopped")?
        }

        async fn prepare(&self, config: PythonToolConfig) -> anyhow::Result<PreinitRuntime> {
            let (tx, rx) = oneshot::channel();
            self.tx
                .send(ExecutionJob::Prepare { config, tx })
                .map_err(|_| anyhow::anyhow!("eryx execution queue stopped"))?;
            rx.await.context("eryx worker stopped")?
        }
    }

    enum ExecutionJob {
        Prepare {
            config: PythonToolConfig,
            tx: oneshot::Sender<anyhow::Result<PreinitRuntime>>,
        },
        Execute {
            request: ExecutionRequest,
            tx: oneshot::Sender<anyhow::Result<ExecutionOutput>>,
        },
        ExecuteWithTools {
            request: ExecutionRequest,
            tools: Vec<PythonToolSpec>,
            calls: tokio::sync::mpsc::UnboundedSender<HostToolCall>,
            tx: oneshot::Sender<anyhow::Result<ExecutionOutput>>,
        },
    }

    struct ExecutionRequest {
        runtime: Arc<PreinitRuntime>,
        script: String,
        limits: ExecutionLimits,
        volumes: Vec<eryx::VolumeMount>,
        network: NetworkAccess,
        /// Cache directory for downloaded Typst packages. Used by the always-on
        /// `typst` module; unused when the `typst` feature is off.
        #[cfg_attr(not(feature = "typst"), allow(dead_code))]
        package_cache: PathBuf,
        /// Shared font cache, mounted read-only into the guest and scanned by the
        /// host Typst compiler. `None` when the download failed.
        fonts_dir: Option<PathBuf>,
    }

    /// Appends the standard per-execution mounts — the shared read-only font
    /// cache (when present) and a writable `/tmp` scratch — to `volumes`.
    fn push_base_mounts(
        volumes: &mut Vec<eryx::VolumeMount>,
        fonts_dir: &Option<PathBuf>,
        scratch: &ScratchTmp,
    ) {
        if let Some(dir) = fonts_dir {
            volumes.push(eryx::VolumeMount::read_only(dir, crate::FONTS_GUEST_DIR));
        }
        volumes.push(scratch.volume());
    }

    fn worker_loop(rx: Arc<Mutex<mpsc::Receiver<ExecutionJob>>>) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("eryx worker runtime");

        loop {
            let job = {
                let rx = rx.lock().expect("eryx execution queue poisoned");
                rx.recv()
            };
            let Ok(job) = job else {
                break;
            };
            match job {
                ExecutionJob::Prepare { config, tx } => {
                    let result = runtime.block_on(build_runtime(config));
                    let _ = tx.send(result);
                }
                ExecutionJob::Execute { request, tx } => {
                    let result = runtime.block_on(execute_request(request));
                    let _ = tx.send(result);
                }
                ExecutionJob::ExecuteWithTools {
                    request,
                    tools,
                    calls,
                    tx,
                } => {
                    let result =
                        runtime.block_on(execute_request_with_tools(request, tools, calls));
                    let _ = tx.send(result);
                }
            }
        }
    }

    /// The always-on built-in Python modules (currently just `typst`) attached
    /// to every execution: their host callbacks plus the preamble exposing them.
    /// Empty when the `typst` feature is disabled.
    fn builtin_modules(request: &ExecutionRequest) -> (Vec<Arc<dyn eryx::Callback>>, String) {
        #[cfg(feature = "typst")]
        {
            let allow_network = matches!(request.network, NetworkAccess::Allowed);
            let font_paths = request.fonts_dir.clone().into_iter().collect();
            let callback = crate::typst_bridge::callback(
                Some(request.package_cache.clone()),
                font_paths,
                allow_network,
            );
            (vec![callback], crate::typst_bridge::PREAMBLE.to_string())
        }
        #[cfg(not(feature = "typst"))]
        {
            let _ = request;
            (Vec::new(), String::new())
        }
    }

    async fn execute_request(request: ExecutionRequest) -> anyhow::Result<ExecutionOutput> {
        let (callbacks, preamble) = builtin_modules(&request);
        if matches!(request.network, NetworkAccess::Allowed) {
            return run_networked(request, callbacks, preamble).await;
        }
        if callbacks.is_empty() {
            return run_plain(request).await;
        }
        run_local(request, callbacks, preamble).await
    }

    /// Fast path with no host callbacks: run the script directly on the cached,
    /// preinitialized interpreter.
    async fn run_plain(request: ExecutionRequest) -> anyhow::Result<ExecutionOutput> {
        // Keep the scratch dir alive for the whole execution.
        let scratch = ScratchTmp::new()?;
        let mut volumes = request.volumes;
        push_base_mounts(&mut volumes, &request.fonts_dir, &scratch);

        let mut execute = request
            .runtime
            .executor
            .execute(request.script)
            .with_timeout(request.limits.max_runtime);
        if let Some(limit) = request.limits.max_memory_bytes {
            execute = execute.with_memory_limit(limit);
        }
        if let Some(fuel) = request.limits.max_cpu_fuel {
            execute = execute.with_fuel_limit(fuel);
        }
        execute = execute.with_volumes(volumes);

        let result = execute.run().await.context("execute eryx script")?;
        drop(scratch);
        Ok(ExecutionOutput {
            stdout: result.stdout,
            stderr: result.stderr,
        })
    }

    /// A host-backed writable `/tmp` for the guest. CPython's `tempfile`,
    /// matplotlib's font cache and similar code expect a writable `/tmp`; the
    /// hybrid VFS only exposes `/data` and the mounted volumes otherwise, so
    /// without this `mkdir`/`open` under `/tmp` fail with ENOENT.
    struct ScratchTmp {
        dir: tempfile::TempDir,
    }

    impl ScratchTmp {
        fn new() -> anyhow::Result<Self> {
            Ok(Self {
                dir: tempfile::tempdir().context("create eryx /tmp scratch dir")?,
            })
        }

        fn volume(&self) -> eryx::VolumeMount {
            eryx::VolumeMount::new(self.dir.path(), "/tmp")
        }
    }

    /// Network-enabled path: a fresh Sandbox is required to wire the network
    /// handler, so host callbacks and the preamble are attached through a
    /// `RuntimeLibrary`. With no callbacks and an empty preamble this is a plain
    /// networked run.
    async fn run_networked(
        request: ExecutionRequest,
        callbacks: Vec<Arc<dyn eryx::Callback>>,
        preamble: String,
    ) -> anyhow::Result<ExecutionOutput> {
        let scratch = ScratchTmp::new()?;
        let mut volumes = request.volumes;
        push_base_mounts(&mut volumes, &request.fonts_dir, &scratch);

        let mut builder = unsafe {
            eryx::Sandbox::builder()
                .with_precompiled_file(&request.runtime.runtime)
                .with_python_stdlib(&request.runtime.stdlib)
        }
        .with_resource_limits(to_eryx_limits(&request.limits))
        .with_volumes(volumes)
        .with_network(eryx::NetConfig::permissive());
        if !callbacks.is_empty() || !preamble.is_empty() {
            let library = eryx::RuntimeLibrary::new()
                .with_callbacks(callbacks.into_iter().map(boxed_callback).collect())
                .with_preamble(preamble);
            builder = builder.with_library(library);
        }
        if let Some(site_packages) = request.runtime.site_packages.as_ref() {
            builder = builder.with_site_packages(site_packages);
        }
        let sandbox = builder.build().context("build eryx sandbox")?;
        let result = sandbox
            .execute(&request.script)
            .await
            .context("execute eryx script")?;
        drop(scratch);
        Ok(ExecutionOutput {
            stdout: result.stdout,
            stderr: result.stderr,
        })
    }

    fn to_eryx_limits(limits: &ExecutionLimits) -> eryx::ResourceLimits {
        eryx::ResourceLimits {
            execution_timeout: Some(limits.max_runtime),
            max_memory_bytes: limits.max_memory_bytes,
            max_fuel: limits.max_cpu_fuel,
            ..Default::default()
        }
    }

    async fn execute_request_with_tools(
        request: ExecutionRequest,
        tools: Vec<PythonToolSpec>,
        calls: tokio::sync::mpsc::UnboundedSender<HostToolCall>,
    ) -> anyhow::Result<ExecutionOutput> {
        let (mut callbacks, mut preamble) = builtin_modules(&request);
        let (tool_callbacks, tool_preamble) = build_tool_callbacks(&tools, calls);
        callbacks.extend(tool_callbacks);
        if !tool_preamble.is_empty() {
            if !preamble.is_empty() {
                preamble.push('\n');
            }
            preamble.push_str(&tool_preamble);
        }

        if matches!(request.network, NetworkAccess::Allowed) {
            return run_networked(request, callbacks, preamble).await;
        }
        run_local(request, callbacks, preamble).await
    }

    /// Fast path that attaches host callbacks to the cached interpreter and
    /// services them on a side task.
    async fn run_local(
        request: ExecutionRequest,
        callbacks: Vec<Arc<dyn eryx::Callback>>,
        preamble: String,
    ) -> anyhow::Result<ExecutionOutput> {
        let scratch = ScratchTmp::new()?;
        let mut volumes = request.volumes;
        push_base_mounts(&mut volumes, &request.fonts_dir, &scratch);

        let dispatch: HashMap<String, Arc<dyn eryx::Callback>> = callbacks
            .iter()
            .map(|cb| (cb.name().to_string(), cb.clone()))
            .collect();
        let (cb_tx, cb_rx) = tokio::sync::mpsc::channel::<eryx::CallbackRequest>(32);
        let handler = tokio::spawn(eryx::callback_handler::run_callback_handler(
            cb_rx,
            Arc::new(dispatch),
            unlimited_callback_limits(),
            Arc::new(HashMap::new()),
        ));

        let full_code = format!("{preamble}\n{}", request.script);
        let mut execute = request
            .runtime
            .executor
            .execute(full_code)
            .with_callbacks(&callbacks, cb_tx)
            .with_timeout(request.limits.max_runtime);
        if let Some(limit) = request.limits.max_memory_bytes {
            execute = execute.with_memory_limit(limit);
        }
        if let Some(fuel) = request.limits.max_cpu_fuel {
            execute = execute.with_fuel_limit(fuel);
        }
        execute = execute.with_volumes(volumes);

        let result = execute.run().await.context("execute eryx script");
        handler.abort();
        drop(scratch);
        let result = result?;
        Ok(ExecutionOutput {
            stdout: result.stdout,
            stderr: result.stderr,
        })
    }

    /// `Arc<dyn Callback>` cannot be turned into `Box<dyn Callback>`, so wrap it
    /// in a thin forwarding type for `RuntimeLibrary::with_callbacks`.
    fn boxed_callback(callback: Arc<dyn eryx::Callback>) -> Box<dyn eryx::Callback> {
        Box::new(SharedCallback(callback))
    }

    struct SharedCallback(Arc<dyn eryx::Callback>);

    impl eryx::Callback for SharedCallback {
        fn name(&self) -> &str {
            self.0.name()
        }

        fn description(&self) -> &str {
            self.0.description()
        }

        fn parameters_schema(&self) -> eryx::Schema {
            self.0.parameters_schema()
        }

        fn invoke(
            &self,
            args: serde_json::Value,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<serde_json::Value, eryx::CallbackError>>
                    + Send
                    + '_,
            >,
        > {
            self.0.invoke(args)
        }
    }

    /// Callback limits for the host-side tool dispatcher. The host tools are
    /// trusted, so neither invocation count nor per-call timeout is capped here;
    /// script-level limits still bound the overall execution.
    fn unlimited_callback_limits() -> eryx::ResourceLimits {
        eryx::ResourceLimits {
            callback_timeout: None,
            max_callback_invocations: None,
            ..Default::default()
        }
    }

    /// Build one tool callback plus its `(attribute, callback_name)` mapping for
    /// the Python preamble.
    fn tool_callback(
        index: usize,
        spec: &PythonToolSpec,
        calls: tokio::sync::mpsc::UnboundedSender<HostToolCall>,
    ) -> (Arc<dyn eryx::Callback>, (String, String)) {
        let callback_name = format!("__tool_{index}");
        let attribute = python_attribute(&spec.name);
        let schema = eryx::Schema::try_from_value(spec.parameters.clone())
            .or_else(|_| eryx::Schema::try_from_value(json!({ "type": "object" })))
            .unwrap_or_else(|_| eryx::Schema::empty());
        let tool_name = spec.name.clone();
        let callback = eryx::DynamicCallback::builder(
            callback_name.clone(),
            spec.description.clone(),
            move |args| {
                let calls = calls.clone();
                let tool_name = tool_name.clone();
                Box::pin(async move {
                    let (reply, response) = tokio::sync::oneshot::channel();
                    calls
                        .send(HostToolCall {
                            name: tool_name,
                            args,
                            reply,
                        })
                        .map_err(|_| {
                            eryx::CallbackError::ExecutionFailed(
                                "host tool channel closed".to_string(),
                            )
                        })?;
                    response.await.map_err(|_| {
                        eryx::CallbackError::ExecutionFailed(
                            "host tool dispatch dropped".to_string(),
                        )
                    })
                })
            },
        )
        .schema(schema)
        .build();
        (Arc::new(callback), (attribute, callback_name))
    }

    /// Build the tool callbacks and the Python preamble that exposes them under
    /// the `tools` package.
    fn build_tool_callbacks(
        tools: &[PythonToolSpec],
        calls: tokio::sync::mpsc::UnboundedSender<HostToolCall>,
    ) -> (Vec<Arc<dyn eryx::Callback>>, String) {
        let mut callbacks = Vec::with_capacity(tools.len());
        let mut entries = Vec::with_capacity(tools.len());
        for (index, spec) in tools.iter().enumerate() {
            let (callback, entry) = tool_callback(index, spec, calls.clone());
            callbacks.push(callback);
            entries.push(entry);
        }
        (callbacks, build_tools_preamble(&entries))
    }

    use crate::python_attribute;

    /// Python preamble registering the `tools` package (and global) whose members
    /// forward to the registered callbacks via `invoke`.
    fn build_tools_preamble(entries: &[(String, String)]) -> String {
        let mut mapping = String::new();
        for (attribute, callback) in entries {
            mapping.push_str(&format!("    ({attribute:?}, {callback:?}),\n"));
        }
        format!(
            "import sys as _sys, types as _types\n\
             _tools_pkg = _types.ModuleType(\"tools\")\n\
             def _tools_make(_cb):\n\
             \x20   async def _call(**kwargs):\n\
             \x20       return await invoke(_cb, **kwargs)\n\
             \x20   return _call\n\
             for _attr, _cb in [\n{mapping}]:\n\
             \x20   setattr(_tools_pkg, _attr, _tools_make(_cb))\n\
             _sys.modules[\"tools\"] = _tools_pkg\n\
             tools = _tools_pkg\n"
        )
    }
}

/// Sanitize a tool name into a valid Python identifier for the `tools` package
/// attribute. Tool names are normally already valid; this guards MCP names.
#[cfg_attr(not(feature = "eryx"), allow(dead_code))]
fn python_attribute(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

#[cfg(feature = "eryx")]
pub async fn prepare_eryx_runtime(config: PythonToolConfig) -> anyhow::Result<()> {
    eryx_backend::prepare_runtime(config).await
}

#[cfg(not(feature = "eryx"))]
pub async fn prepare_eryx_runtime(_config: PythonToolConfig) -> anyhow::Result<()> {
    anyhow::bail!("execenv was built without the eryx feature")
}

#[cfg(feature = "eryx")]
async fn make_eryx_service(
    config: PythonToolConfig,
    fs: Arc<dyn FileSystemBackend>,
) -> anyhow::Result<Arc<dyn ExecutionService>> {
    Ok(Arc::new(
        eryx_backend::EryxExecutionService::new(config, fs).await?,
    ))
}

#[cfg(not(feature = "eryx"))]
async fn make_eryx_service(
    _config: PythonToolConfig,
    _fs: Arc<dyn FileSystemBackend>,
) -> anyhow::Result<Arc<dyn ExecutionService>> {
    anyhow::bail!("execenv was built without the eryx feature")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "network: downloads a tarball through a GitHub release redirect"]
    async fn download_follows_redirects_and_fetches_tarball() {
        // A GitHub release asset that 302-redirects to release-assets storage,
        // exercising the redirect-following path of `download`.
        let url = NUMPY_URL;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("numpy-wasi.tar.gz");
        artifacts::download(url, &path).await.unwrap();
        let bytes = tokio::fs::read(&path).await.unwrap();
        assert!(bytes.len() > 1024, "got {} bytes", bytes.len());
        assert_eq!(&bytes[..2], &[0x1f, 0x8b], "not a gzip stream");
    }

    #[test]
    fn package_manifest_is_well_formed() {
        let mut names = std::collections::HashSet::new();
        for pkg in WASI_PACKAGES {
            assert!(
                names.insert(pkg.name),
                "duplicate package name {}",
                pkg.name
            );
            assert!(
                pkg.url.starts_with("https://"),
                "{} url not https",
                pkg.name
            );
            assert_eq!(pkg.sha256.len(), 64, "{} has invalid sha256", pkg.name);
        }
    }

    #[tokio::test]
    async fn mock_service_returns_empty_output() {
        let output = MockExecutionService
            .execute_python("print('ignored')")
            .await
            .unwrap();

        assert_eq!(
            output,
            ExecutionOutput {
                stdout: String::new(),
                stderr: String::new()
            }
        );
    }

    #[tokio::test]
    async fn python_tool_wraps_service_output() {
        struct EchoService;

        #[async_trait]
        impl ExecutionService for EchoService {
            async fn execute_python(&self, script: &str) -> anyhow::Result<ExecutionOutput> {
                Ok(ExecutionOutput {
                    stdout: script.to_string(),
                    stderr: String::new(),
                })
            }
        }

        let tool = PythonTool::from_service(Arc::new(EchoService));
        let result = tool
            .execute(
                Arc::new(AgentConfig {
                    model_registry: stride_agent::ModelRegistry::new(),
                    max_iterations: 1,
                    usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                    ..Default::default()
                }),
                json!({ "script": "print(1)" }),
            )
            .await;

        assert_eq!(result["success"], true, "{result}");
        assert_eq!(result["stdout"], "print(1)");
    }

    struct TestTool {
        name: &'static str,
        confirm: bool,
    }

    #[async_trait(?Send)]
    impl Tool for TestTool {
        fn name(&self) -> &str {
            self.name
        }

        fn readable_name(&self) -> &str {
            self.name
        }

        fn definition(&self) -> LlmTool {
            LlmTool {
                r#type: llm::ToolType::Function,
                function: Function {
                    description: format!("{} tool", self.name),
                    name: self.name.to_string(),
                    parameters: Some(llm::FunctionParameters {
                        param_type: "object".to_string(),
                        ..Default::default()
                    }),
                },
            }
        }

        async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
            json!({ "echoed": args })
        }

        fn requires_confirmation(&self) -> bool {
            self.confirm
        }
    }

    fn test_config() -> Arc<AgentConfig> {
        Arc::new(AgentConfig {
            model_registry: stride_agent::ModelRegistry::new(),
            max_iterations: 1,
            usage_observer: Arc::new(stride_agent::NoopUsageObserver),
            ..Default::default()
        })
    }

    #[test]
    fn with_tools_exposes_only_auto_approved_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(TestTool {
            name: "echo",
            confirm: false,
        });
        registry.register(TestTool {
            name: "danger",
            confirm: true,
        });
        registry.register(TestTool {
            name: "allowed",
            confirm: true,
        });
        registry.allow_tool("allowed");

        let tool = PythonTool::from_service(Arc::new(MockExecutionService)).with_tools(registry);
        let names: Vec<&str> = tool.specs.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"allowed"));
        assert!(!names.contains(&"danger"));
    }

    #[tokio::test]
    async fn execute_with_tools_dispatches_calls_to_registry() {
        // A service that simulates a script invoking the `echo` tool once.
        struct CallingService;

        #[async_trait]
        impl ExecutionService for CallingService {
            async fn execute_python(&self, _script: &str) -> anyhow::Result<ExecutionOutput> {
                unreachable!("tools path should be used")
            }

            async fn execute_python_with_tools(
                &self,
                _script: &str,
                tools: &[PythonToolSpec],
                calls: mpsc::UnboundedSender<HostToolCall>,
            ) -> anyhow::Result<ExecutionOutput> {
                assert!(tools.iter().any(|t| t.name == "echo"));
                let (reply, response) = oneshot::channel();
                calls
                    .send(HostToolCall {
                        name: "echo".to_string(),
                        args: json!({ "msg": "hi" }),
                        reply,
                    })
                    .map_err(|_| anyhow::anyhow!("send"))?;
                let result = response.await?;
                Ok(ExecutionOutput {
                    stdout: result.to_string(),
                    stderr: String::new(),
                })
            }
        }

        let mut registry = ToolRegistry::new();
        registry.register(TestTool {
            name: "echo",
            confirm: false,
        });
        let tool = PythonTool::from_service(Arc::new(CallingService)).with_tools(registry);

        let result = tool.execute(test_config(), json!({ "script": "x" })).await;
        assert_eq!(result["success"], true, "{result}");
        let stdout = result["stdout"].as_str().unwrap();
        assert!(stdout.contains("echoed"), "{stdout}");
        assert!(stdout.contains("hi"), "{stdout}");
    }

    #[test]
    fn python_attribute_sanitizes_non_identifiers() {
        assert_eq!(python_attribute("web_search"), "web_search");
        assert_eq!(python_attribute("github.search-code"), "github_search_code");
        assert_eq!(python_attribute("3d"), "_3d");
    }

    #[cfg(feature = "eryx")]
    #[tokio::test]
    #[ignore = "precompiles Eryx runtime"]
    async fn eryx_backend_executes_base_python_without_numpy() {
        let workspace = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let fs = Arc::new(
            DirectOsFileSystem::new(workspace.path().join("workspace"))
                .unwrap()
                .guest_dir("/~workspace"),
        );
        let config = PythonToolConfig {
            cache_dir: cache.path().to_path_buf(),
            backend: BackendKind::Eryx,
            threads: 2,
            preinit: false,
            limits: ExecutionLimits {
                max_runtime: Duration::from_secs(5),
                max_memory_bytes: Some(128 * 1024 * 1024),
                max_cpu_fuel: None,
            },
            network: NetworkAccess::Blocked,
        };
        let tool = PythonTool::new(config, fs).await.unwrap();

        let result = tool
            .execute(
                Arc::new(AgentConfig {
                    model_registry: stride_agent::ModelRegistry::new(),
                    max_iterations: 1,
                    usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                    ..Default::default()
                }),
                json!({ "script": "print(2 + 2)" }),
            )
            .await;

        assert_eq!(result["success"], true, "{result}");
        assert_eq!(result["stdout"].as_str().unwrap().trim(), "4");
    }

    /// Read-only volume for the whole filesystem plus a read-write volume nested
    /// inside it — the layout `VfsExecFileSystem` uses for a project thread.
    #[cfg(feature = "eryx")]
    struct TwoMountFs {
        root: std::path::PathBuf,
        rw: std::path::PathBuf,
        guest_rw: String,
    }

    #[cfg(feature = "eryx")]
    impl FileSystemBackend for TwoMountFs {
        fn volumes(&self) -> Vec<VolumeMount> {
            vec![
                VolumeMount::read_only(&self.root, "/"),
                VolumeMount::new(&self.rw, &self.guest_rw),
            ]
        }
    }

    #[cfg(feature = "eryx")]
    #[tokio::test]
    #[ignore = "precompiles Eryx runtime"]
    async fn eryx_mounts_whole_fs_readonly_with_writable_subtree() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let rw = tmp.path().join("rw");
        // The read-only world, including a read-only copy of the project folder.
        std::fs::create_dir_all(root.join("Projects/Acme")).unwrap();
        std::fs::write(root.join("other.txt"), "GLOBALDATA").unwrap();
        std::fs::write(root.join("Projects/Acme/existing.txt"), "ROOTCOPY").unwrap();
        // The writable mirror of the project folder, mounted on top.
        std::fs::create_dir_all(&rw).unwrap();
        std::fs::write(rw.join("existing.txt"), "RWCOPY").unwrap();

        let fs = Arc::new(TwoMountFs {
            root,
            rw: rw.clone(),
            guest_rw: "/Projects/Acme".to_string(),
        });
        let config = PythonToolConfig {
            cache_dir: cache.path().to_path_buf(),
            backend: BackendKind::Eryx,
            threads: 2,
            preinit: false,
            limits: ExecutionLimits {
                max_runtime: Duration::from_secs(10),
                max_memory_bytes: Some(128 * 1024 * 1024),
                max_cpu_fuel: None,
            },
            network: NetworkAccess::Blocked,
        };
        let tool = PythonTool::new(config, fs).await.unwrap();

        let script = concat!(
            "print('READ_GLOBAL', open('/other.txt').read())\n",
            "print('READ_NESTED', open('/Projects/Acme/existing.txt').read())\n",
            "open('/Projects/Acme/new.txt', 'w').write('NEW')\n",
            "print('WROTE_RW', open('/Projects/Acme/new.txt').read())\n",
            "try:\n",
            "    open('/blocked.txt', 'w').write('x')\n",
            "    print('RO_WRITE_UNEXPECTED')\n",
            "except Exception:\n",
            "    print('RO_WRITE_BLOCKED')\n",
        );

        let result = tool
            .execute(
                Arc::new(AgentConfig {
                    model_registry: stride_agent::ModelRegistry::new(),
                    max_iterations: 1,
                    usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                    ..Default::default()
                }),
                json!({ "script": script }),
            )
            .await;

        assert_eq!(result["success"], true, "{result}");
        let stdout = result["stdout"].as_str().unwrap();
        // The whole filesystem is readable.
        assert!(stdout.contains("READ_GLOBAL GLOBALDATA"), "{stdout}");
        // The writable mount shadows the read-only copy at the same path.
        assert!(stdout.contains("READ_NESTED RWCOPY"), "{stdout}");
        // Writes land in the writable subtree.
        assert!(stdout.contains("WROTE_RW NEW"), "{stdout}");
        // Writes elsewhere are denied by the read-only root mount.
        assert!(stdout.contains("RO_WRITE_BLOCKED"), "{stdout}");
        assert_eq!(std::fs::read_to_string(rw.join("new.txt")).unwrap(), "NEW");
    }

    #[cfg(feature = "eryx")]
    #[tokio::test]
    #[ignore = "downloads pure-Python wheels and precompiles runtime"]
    async fn eryx_backend_imports_pure_python_packages() {
        let workspace = tempfile::tempdir().unwrap();
        let cache_dir = std::env::temp_dir().join("stride-execenv-pure-test-cache");
        tokio::fs::create_dir_all(&cache_dir).await.unwrap();
        let fs = Arc::new(
            DirectOsFileSystem::new(workspace.path().join("workspace"))
                .unwrap()
                .guest_dir("/~workspace"),
        );
        let config = PythonToolConfig {
            cache_dir,
            backend: BackendKind::Eryx,
            threads: 1,
            preinit: true,
            limits: ExecutionLimits::default(),
            network: NetworkAccess::Blocked,
        };
        prepare_eryx_runtime(config.clone()).await.unwrap();
        let tool = PythonTool::new(config, fs).await.unwrap();

        // No network needed: exercise imports and offline behaviour only.
        let script = "import markdown, dateutil, requests\n\
             from bs4 import BeautifulSoup\n\
             html = markdown.markdown('# Title')\n\
             text = BeautifulSoup(html, 'html.parser').get_text().strip()\n\
             print(text)";
        let result = tool
            .execute(
                Arc::new(AgentConfig {
                    model_registry: stride_agent::ModelRegistry::new(),
                    max_iterations: 1,
                    usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                    ..Default::default()
                }),
                json!({ "script": script }),
            )
            .await;

        assert_eq!(result["success"], true, "{result}");
        assert_eq!(result["stdout"].as_str().unwrap().trim(), "Title");
    }

    #[cfg(feature = "eryx")]
    #[tokio::test]
    #[ignore = "downloads native WASI packages and precompiles runtime"]
    async fn eryx_backend_imports_native_packages() {
        let workspace = tempfile::tempdir().unwrap();
        let cache_dir = std::env::temp_dir().join("stride-execenv-native-test-cache");
        tokio::fs::create_dir_all(&cache_dir).await.unwrap();
        let fs = Arc::new(
            DirectOsFileSystem::new(workspace.path().join("workspace"))
                .unwrap()
                .guest_dir("/~workspace"),
        );
        let config = PythonToolConfig {
            cache_dir,
            backend: BackendKind::Eryx,
            threads: 1,
            preinit: true,
            limits: ExecutionLimits::default(),
            network: NetworkAccess::Blocked,
        };
        prepare_eryx_runtime(config.clone()).await.unwrap();
        let tool = PythonTool::new(config.clone(), fs).await.unwrap();

        // Exercises native imports, a writable /tmp (matplotlib font cache lands
        // there) and saving a figure into the /~workspace mount.
        let script = "import numpy as np\n\
             import pandas as pd\n\
             from PIL import Image\n\
             import matplotlib\n\
             matplotlib.use('Agg')\n\
             import matplotlib.pyplot as plt\n\
             total = int(np.arange(5).sum())\n\
             rows = len(pd.DataFrame({'a': [1, 2, 3]}))\n\
             size = Image.new('RGB', (4, 2)).size\n\
             plt.plot([1, 2, 3], [3, 1, 2])\n\
             plt.savefig('/~workspace/sample_plot.png')\n\
             print(total, rows, size[0])";
        let result = tool
            .execute(
                Arc::new(AgentConfig {
                    model_registry: stride_agent::ModelRegistry::new(),
                    max_iterations: 1,
                    usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                    ..Default::default()
                }),
                json!({ "script": script }),
            )
            .await;

        assert_eq!(result["success"], true, "{result}");
        assert_eq!(result["stdout"].as_str().unwrap().trim(), "10 3 4");
        let png = workspace.path().join("workspace").join("sample_plot.png");
        assert!(png.exists(), "savefig did not write to /~workspace");
    }

    #[cfg(all(feature = "eryx", feature = "typst"))]
    #[tokio::test]
    #[ignore = "downloads the CPython WASI stdlib and precompiles the runtime"]
    async fn eryx_typst_module_compiles_pdf() {
        let workspace = tempfile::tempdir().unwrap();
        let ws_dir = workspace.path().join("workspace");
        std::fs::create_dir_all(&ws_dir).unwrap();
        std::fs::write(ws_dir.join("doc.typ"), b"= Title\nHello from Typst").unwrap();

        let cache_dir = std::env::temp_dir().join("stride-execenv-typst-test-cache");
        tokio::fs::create_dir_all(&cache_dir).await.unwrap();
        let fs = Arc::new(
            DirectOsFileSystem::new(ws_dir.clone())
                .unwrap()
                .guest_dir("/~workspace"),
        );
        let config = PythonToolConfig {
            cache_dir,
            backend: BackendKind::Eryx,
            threads: 1,
            preinit: true,
            limits: ExecutionLimits::default(),
            network: NetworkAccess::Blocked,
        };
        prepare_eryx_runtime(config.clone()).await.unwrap();
        let tool = PythonTool::new(config, fs).await.unwrap();

        // Exercises the in-sandbox `typst` module: gather the workspace project,
        // ship it to the host compiler over the callback bridge, decode the
        // returned PDF bytes and write them back to the mounted workspace.
        let script = "import typst\n\
             await typst.compile('/~workspace/doc.typ', output='/~workspace/doc.pdf')\n\
             data = await typst.compile('/~workspace/doc.typ')\n\
             print(len(data), data[:5].decode('latin1'))";
        let result = tool
            .execute(
                Arc::new(AgentConfig {
                    model_registry: stride_agent::ModelRegistry::new(),
                    max_iterations: 1,
                    usage_observer: Arc::new(stride_agent::NoopUsageObserver),
                    ..Default::default()
                }),
                json!({ "script": script }),
            )
            .await;

        assert_eq!(result["success"], true, "{result}");
        assert!(
            result["stdout"].as_str().unwrap().contains("%PDF-"),
            "{result}"
        );
        let pdf = ws_dir.join("doc.pdf");
        assert!(
            pdf.exists(),
            "typst.compile(output=...) did not write the pdf"
        );
        assert_eq!(&std::fs::read(pdf).unwrap()[..5], b"%PDF-");
    }
}
