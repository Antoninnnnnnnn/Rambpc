# Rambler native access — bounded public-documentation research

Date: 2026-11-17. Source baseline specified: `ee336cf309bb8cd8e654a435e0a85ce24632f8fb`.
Scope: public Google documentation on legitimate provisioning for the exact Gboard/Rambler S3 backend, not substitute products. Starting references: `docs/research/authentication.md` and `docs/research/protocol.md` in the project. Those references report the observed host `speechs3proto2-pa.googleapis.com:443` and gRPC method `/speech.s3.S3StubbyClientConnectorService/S3Bidi`; their observation does not itself establish authorization. No endpoint or discovery call was made.

## Outcome

The bounded search and two authoritative pages read did **not identify a published native third-party provisioning path, service entitlement, or activation contract for that exact backend**. This is a limited search result, not proof that no such path exists. It does not establish that any particular supplied configuration is authorized or unauthorized. Historical Windows-profile successes remain observations of that specific historical configuration, not proof of provisioning rights; current real callers were not invoked.

## Searches performed

Exact queries, 2026-11-17 (search results are discovery, not independent entitlement evidence):

1. `"speechs3proto2-pa.googleapis.com" "S3Bidi" provisioning Gboard Google` — no links returned.
2. `site:support.google.com/gboard "voice typing" Gboard availability` — returned the Gboard Help Rambler page among others.
3. `site:developers.google.com "Rambler" "Gboard" API developer access` — no links returned.
4. `Google Rambler Gboard third party developer API licensing access native speech S3` — returned the Rambler help page and general Google APIs terms.
5. `site:developers.google.com "speechs3proto2-pa.googleapis.com"` — no links returned.
6. `site:cloud.google.com OR site:developers.google.com "S3StubbyClientConnectorService"` — no links returned.

Search-index coverage and query wording limit these negative results. No other authoritative pages were opened; generic API-key/restriction pages already covered in the starting documentation were not refetched.

## Authoritative pages read

1. Google, **Rambler voice input on Gboard**, https://support.google.com/gboard/answer/17468539?hl=en (accessed 2026-11-17). The user-facing product page lists Pixel 11 series, latest Gboard set as default, microphone permission and an active internet connection for full functionality; it states “Rambler is subject to usage limits.” It describes Gboard use, not the identified S3 method, a third-party native client, provisioning or an entitlement contract. Its silence is not a third-party-access prohibition and does not establish that the observed backend is the one used by every configuration of the documented product.
2. Google for Developers, **Google APIs Terms of Service**, https://developers.google.com/terms (accessed 2026-11-17), section 2(c): “You will only access (or attempt to access) an API by the means described in the documentation of that API.” Section 2(d) addresses API limits and consent for exceeding them. The terms do not identify Gboard/Rambler S3 or supply a provisioning mechanism for it; applicability and any service-specific terms for the exact backend remain unestablished. They cannot alone adjudicate a particular configuration.

## Missing evidence and next gate

The remaining blocker is **server/access evidence**, not a finding that a local code fix can confer access. Obtain a verifiable, nonsecret provider-issued or provider-controlled entitlement/activation record, or authoritative service-specific published contract, explicitly covering: the exact Rambler/S3 service and intended native client/platform; the entitled organization/project/application and permitted use; how its configuration is legitimately issued and bound to that client; applicable service-specific terms, lifecycle and restrictions; and the responsible issuer/provenance. Confirm that it covers the configuration actually supplied without exposing its values. If no public contract exists, that evidence must come from the service owner or an authorized provisioning channel; this research did not contact either. A general permission for another call or a per-call budget is not a substitute for this evidence.

**Concrete locally fixable issues:** none identified by this documentation-only investigation; no claim is made about other code defects. No code, tests, builds, device actions, service calls, configuration values, or private data were inspected or executed for this report.
