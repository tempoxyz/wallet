import Foundation
import Security

// MARK: - Helpers

func hexEncode(_ data: Data) -> String {
    data.map { String(format: "%02x", $0) }.joined()
}

func hexDecode(_ hex: String) -> Data? {
    var data = Data()
    var chars = hex[hex.startIndex...]
    while chars.count >= 2 {
        let end = chars.index(chars.startIndex, offsetBy: 2)
        guard let byte = UInt8(chars[chars.startIndex..<end], radix: 16) else { return nil }
        data.append(byte)
        chars = chars[end...]
    }
    return chars.isEmpty ? data : nil
}

func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data((message + "\n").utf8))
    exit(1)
}

func cfError(_ status: OSStatus) -> String {
    if let msg = SecCopyErrorMessageString(status, nil) as String? {
        return msg
    }
    return "OSStatus \(status)"
}

// MARK: - Secure Enclave operations

func generateKey(tag: String) -> Data {
    let tagData = Data(tag.utf8)

    guard let access = SecAccessControlCreateWithFlags(
        kCFAllocatorDefault,
        kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
        .privateKeyUsage,
        nil
    ) else {
        fail("Failed to create access control")
    }

    let attributes: [String: Any] = [
        kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
        kSecAttrKeySizeInBits as String: 256,
        kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
        kSecPrivateKeyAttrs as String: [
            kSecAttrIsPermanent as String: true,
            kSecAttrApplicationTag as String: tagData,
            kSecAttrAccessControl as String: access,
        ] as [String: Any],
    ]

    var error: Unmanaged<CFError>?
    guard let privateKey = SecKeyCreateRandomKey(attributes as CFDictionary, &error) else {
        let desc = error?.takeRetainedValue().localizedDescription ?? "unknown error"
        fail("Failed to generate SE key: \(desc)")
    }

    guard let publicKey = SecKeyCopyPublicKey(privateKey) else {
        fail("Failed to extract public key")
    }

    var exportError: Unmanaged<CFError>?
    guard let pubData = SecKeyCopyExternalRepresentation(publicKey, &exportError) as Data? else {
        let desc = exportError?.takeRetainedValue().localizedDescription ?? "unknown error"
        fail("Failed to export public key: \(desc)")
    }

    return pubData
}

func loadPrivateKey(tag: String) -> SecKey {
    let tagData = Data(tag.utf8)

    let query: [String: Any] = [
        kSecClass as String: kSecClassKey,
        kSecAttrApplicationTag as String: tagData,
        kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
        kSecAttrKeyClass as String: kSecAttrKeyClassPrivate,
        kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
        kSecMatchLimit as String: kSecMatchLimitOne,
        kSecReturnRef as String: true,
    ]

    var item: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &item)
    guard status == errSecSuccess, let key = item else {
        fail("Failed to load SE key '\(tag)': \(cfError(status))")
    }

    return key as! SecKey
}

func signHash(tag: String, hashHex: String) -> Data {
    guard let hashData = hexDecode(hashHex), hashData.count == 32 else {
        fail("Invalid hash: expected 64 hex characters (32 bytes)")
    }

    let privateKey = loadPrivateKey(tag: tag)

    var error: Unmanaged<CFError>?
    // Use digest variant: input is already a 32-byte SHA-256 hash.
    // .ecdsaSignatureMessageX962SHA256 would double-hash the input.
    guard let signature = SecKeyCreateSignature(
        privateKey,
        .ecdsaSignatureDigestX962SHA256,
        hashData as CFData,
        &error
    ) as Data? else {
        let desc = error?.takeRetainedValue().localizedDescription ?? "unknown error"
        fail("Failed to sign: \(desc)")
    }

    return signature
}

func getPublicKey(tag: String) -> Data {
    let privateKey = loadPrivateKey(tag: tag)

    guard let publicKey = SecKeyCopyPublicKey(privateKey) else {
        fail("Failed to extract public key")
    }

    var error: Unmanaged<CFError>?
    guard let pubData = SecKeyCopyExternalRepresentation(publicKey, &error) as Data? else {
        let desc = error?.takeRetainedValue().localizedDescription ?? "unknown error"
        fail("Failed to export public key: \(desc)")
    }

    return pubData
}

func deleteKey(tag: String) {
    let tagData = Data(tag.utf8)

    let query: [String: Any] = [
        kSecClass as String: kSecClassKey,
        kSecAttrApplicationTag as String: tagData,
        kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
        kSecAttrKeyClass as String: kSecAttrKeyClassPrivate,
        kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
    ]

    let status = SecItemDelete(query as CFDictionary)
    guard status == errSecSuccess || status == errSecItemNotFound else {
        fail("Failed to delete SE key '\(tag)': \(cfError(status))")
    }
}

// MARK: - CLI

let args = CommandLine.arguments

guard args.count >= 2 else {
    fail("Usage: tempo-se <generate|sign|pubkey|delete> --tag <tag> [--hash <hex>]")
}

let command = args[1]

func requireArg(_ flag: String) -> String {
    guard let idx = args.firstIndex(of: flag), idx + 1 < args.count else {
        fail("Missing required argument: \(flag)")
    }
    return args[idx + 1]
}

switch command {
case "generate":
    let tag = requireArg("--tag")
    let pubData = generateKey(tag: tag)
    print(hexEncode(pubData))

case "sign":
    let tag = requireArg("--tag")
    let hashHex = requireArg("--hash")
    let sig = signHash(tag: tag, hashHex: hashHex)
    print(hexEncode(sig))

case "pubkey":
    let tag = requireArg("--tag")
    let pubData = getPublicKey(tag: tag)
    print(hexEncode(pubData))

case "delete":
    let tag = requireArg("--tag")
    deleteKey(tag: tag)

default:
    fail("Unknown command: \(command). Use: generate, sign, pubkey, delete")
}
