// Touch ID signatures for `hivey evolve golive` (user, 2026-11-27). An agent running as the user can fake any check done in
// Python, but it cannot make this signature: the private key lives in the Mac's Secure Enclave, never leaves it, and signs
// only after the user's fingerprint matches (biometry only, no password fallback an agent could type). The public key is
// pinned in the tamper-locked evolve.py, so anyone can verify a go-live later and a faked one shows up in the audit.
//
//   touchid keygen <blob-out>                  make the key (no finger needed); prints the public key (base64 DER)
//   touchid sign <blob> <msg-file>             asks for Touch ID, prints the signature (base64 DER). The prompt is built
//                                              from the message itself (what, which item, which verdict), never from the
//                                              caller, so a caller can't show one thing and sign another.
//   touchid verify <pub-b64> <sig-b64> <msg-file>
//   touchid selftest x                         verify accepts a good signature and refuses a changed message (throwaway
//                                              in-memory key, nothing written); prints its public key and signature
//                                              so the same check can be repeated with /usr/bin/openssl
// Exit 0 = ok/valid, 1 = refused/cancelled/invalid, 2 = unusable (no Touch ID, bad input).
import CryptoKit
import Foundation
import LocalAuthentication

func fail(_ code: Int32, _ msg: String) -> Never {
    FileHandle.standardError.write((msg + "\n").data(using: .utf8)!)
    exit(code)
}

let a = CommandLine.arguments
guard a.count >= 3 else { fail(2, "usage: touchid keygen|sign|verify …") }

switch a[1] {
case "keygen":
    guard SecureEnclave.isAvailable else { fail(2, "no Secure Enclave on this Mac") }
    var cfErr: Unmanaged<CFError>?
    guard let ac = SecAccessControlCreateWithFlags(nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
                                                   [.privateKeyUsage, .biometryCurrentSet], &cfErr) else {
        fail(2, "access control: \(cfErr!.takeRetainedValue())")
    }
    do {
        let key = try SecureEnclave.P256.Signing.PrivateKey(accessControl: ac)
        try key.dataRepresentation.write(to: URL(fileURLWithPath: a[2]), options: .withoutOverwriting)
        print(key.publicKey.derRepresentation.base64EncodedString())
    } catch { fail(2, "keygen: \(error)") }

case "sign":
    guard a.count == 4, let msg = FileManager.default.contents(atPath: a[3]),
          let blob = FileManager.default.contents(atPath: a[2]),
          let j = try? JSONSerialization.jsonObject(with: msg) as? [String: Any],
          let kind = j["kind"] as? String, let item = j["item"] as? String else { fail(2, "sign: bad arguments") }
    let verdict = String(((j["critic_sha"] ?? j["after"]) as? String ?? "?").prefix(12))
    let reason: String
    switch kind {
    case "golive": reason = "approve going live with self-improve item \(item) (answers critic verdict \(verdict))"
    case "round": reason = "allow another critic round for self-improve item \(item) (after verdict \(verdict))"
    default: fail(2, "sign: unknown kind \(kind)")
    }
    let ctx = LAContext()
    ctx.localizedFallbackTitle = ""   // hides "Enter Password"
    ctx.localizedReason = reason
    var err: NSError?
    guard ctx.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &err) else {
        fail(2, "Touch ID unavailable: \(err?.localizedDescription ?? "unknown")")
    }
    do {
        let key = try SecureEnclave.P256.Signing.PrivateKey(dataRepresentation: blob, authenticationContext: ctx)
        print(try key.signature(for: msg).derRepresentation.base64EncodedString())
    } catch { fail(1, "Touch ID refused or cancelled: \(error.localizedDescription)") }

case "verify":
    guard a.count == 5, let msg = FileManager.default.contents(atPath: a[4]),
          let pubD = Data(base64Encoded: a[2]), let sigD = Data(base64Encoded: a[3]) else { fail(2, "verify: bad arguments") }
    do {
        let pub = try P256.Signing.PublicKey(derRepresentation: pubD)
        let sig = try P256.Signing.ECDSASignature(derRepresentation: sigD)
        exit(pub.isValidSignature(sig, for: msg) ? 0 : 1)
    } catch { fail(1, "verify: \(error)") }

case "selftest":
    let k = P256.Signing.PrivateKey()
    let m = Data("{\"item\":\"x\"}".utf8)
    guard let sig = try? k.signature(for: m) else { fail(1, "selftest: could not sign") }
    let good = k.publicKey.isValidSignature(sig, for: m)
    let bad = k.publicKey.isValidSignature(sig, for: Data("{\"item\":\"y\"}".utf8))
    print("good=\(good) changed=\(bad)")
    print(k.publicKey.derRepresentation.base64EncodedString())
    print(sig.derRepresentation.base64EncodedString())
    exit(good && !bad ? 0 : 1)

default:
    fail(2, "usage: touchid keygen|sign|verify …")
}
