use httparse::Header;
use marine_rs_sdk::MountedBinaryStringResult;
use serde::de::DeserializeOwned;

pub struct Http;

impl Http {
    pub fn parse<'a>(
        res: &'a MountedBinaryStringResult,
        headers: &'a mut [Header<'a>],
    ) -> Result<(httparse::Response<'a, 'a>, &'a [u8]), anyhow::Error> {
        log::debug!("Response: {}", res.stdout);
        let mut resp = httparse::Response::new(headers);
        let bytes = res.stdout.as_bytes();
        let sz = resp.parse(bytes)?.unwrap();
        let rest = &bytes[sz..];
        Ok((resp, rest))
    }

    pub fn from<T: DeserializeOwned>(res: MountedBinaryStringResult) -> Result<T, anyhow::Error> {
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let (resp, rest) = Self::parse(&res, &mut headers)?;
        if let Some(200) = resp.code {
            Ok(serde_json::from_slice(rest)?)
        } else {
            Err(anyhow::anyhow!("Error({:?}): {:?}", resp.code, resp.reason))
        }
    }
}
