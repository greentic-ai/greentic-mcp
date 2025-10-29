use crate::config::VerifyPolicy;
use crate::error::VerificationError;
use crate::resolve::ResolvedArtifact;

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct VerifiedArtifact {
    pub resolved: ResolvedArtifact,
    pub verified_digest: Option<String>,
    pub verified_signer: Option<String>,
}

pub fn verify(
    component: &str,
    artifact: ResolvedArtifact,
    policy: &VerifyPolicy,
) -> Result<VerifiedArtifact, VerificationError> {
    if let Some(expected_digest) = policy.required_digests.get(component) {
        if artifact.digest != *expected_digest {
            return Err(VerificationError::DigestMismatch {
                expected: expected_digest.clone(),
                actual: artifact.digest,
            });
        }
    } else if !policy.allow_unverified {
        return Err(VerificationError::UnsignedRejected);
    }

    // Signature verification will be added once the signing infrastructure is finalized.
    Ok(VerifiedArtifact {
        verified_digest: Some(artifact.digest.clone()),
        resolved: artifact,
        verified_signer: None,
    })
}
