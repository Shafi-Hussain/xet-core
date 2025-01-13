#[macro_use]
extern crate napi_derive;

use data::{data_client, PointerFile};
use napi::bindgen_prelude::BigInt;
use once_cell::sync::Lazy;
use std::sync::Arc;
use utils::ThreadPool;

#[napi(object)]
pub struct JsPointerFile {
  pub path: String,
  pub hash: String,
  pub filesize: BigInt,
}

impl From<JsPointerFile> for PointerFile {
  fn from(value: JsPointerFile) -> Self {
    let JsPointerFile {
      path,
      hash,
      filesize,
    } = value;
    let (_, filesize, _) = filesize.get_u64();
    PointerFile::init_from_info(path.as_str(), hash.as_str(), filesize)
  }
}

impl From<PointerFile> for JsPointerFile {
  fn from(value: PointerFile) -> Self {
    Self {
      path: value.path().to_string(),
      hash: value.hash().unwrap().hex(),
      filesize: BigInt::from(value.filesize()),
    }
  }
}

#[napi(object)]
pub struct TokenInfo {
  pub token: String,
  pub expiry: BigInt,
}

const THREADPOOL: Lazy<Arc<ThreadPool>> = Lazy::new(|| Arc::new(ThreadPool::new().expect("")));

// #[pyfunction]
// #[pyo3(signature = (file_paths, endpoint, token_info, token_refresher, progress_updater), text_signature = "(file_paths: List[str], endpoint: Optional[str], token_info: Optional[(str, int)], token_refresher: Optional[Callable[[], (str, int)]], progress_updater: Optional[Callable[[int, None]]) -> List[PyPointerFile]")]
#[napi]
pub async fn upload_files(
  file_paths: Vec<String>,
  endpoint: Option<String>,
  token_info: TokenInfo,
  // token_info: Option<(String, u64)>,
  // token_refresher: Option<Py<PyAny>>,
) -> Result<Vec<JsPointerFile>, napi::Error> {
  // ) -> PyResult<Vec<PyPointerFile>> {
  // let refresher = token_refresher.map(WrappedTokenRefresher::from_func).transpose()?.map(Arc::new);

  let (_, expiry, _) = token_info.expiry.get_u64();
  let token_info = Some((token_info.token, expiry));
  let out: Vec<JsPointerFile> = data_client::upload_async(
    THREADPOOL.clone(),
    file_paths,
    endpoint,
    token_info,
    None, // refresher.map(|v| v as Arc<_>),
    None,
  )
  .await
  .map_err(|e| napi::Error::from_reason(format!("{e}")))?
  .into_iter()
  .map(JsPointerFile::from)
  .collect();
  Ok(out)
}

// #[pyfunction]
// #[pyo3(signature = (files, endpoint, token_info, token_refresher, progress_updater), text_signature = "(files: List[PyPointerFile], endpoint: Optional[str], token_info: Optional[(str, int)], token_refresher: Optional[Callable[[], (str, int)]], progress_updater: Optional[List[Callable[[int], None]]]) -> List[str]"
// )]
#[napi]
pub async fn download_files(
  files: Vec<JsPointerFile>,
  endpoint: Option<String>,
  token_info: TokenInfo,
  // token_refresher: Option<Py<PyAny>>,
) -> Result<Vec<String>, napi::Error> {
  let pfs = files.into_iter().map(PointerFile::from).collect();

  // let refresher = token_refresher.map(WrappedTokenRefresher::from_func).transpose()?.map(Arc::new);

  let (_, expiry, _) = token_info.expiry.get_u64();
  let token_info = Some((token_info.token, expiry));
  let out: Vec<String> = data_client::download_async(
    THREADPOOL.clone(),
    pfs,
    endpoint,
    token_info,
    None, //    refresher.map(|v| v as Arc<_>),
    None,
  )
  .await
  .map_err(|e| napi::Error::from_reason(format!("{e}")))?;

  Ok(out)
}
