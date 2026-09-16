const { randomBytes } = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const {
  certificateFingerprint,
  createSigningIgnore,
  designatedRequirement,
  loadSigningCredentials,
  run,
  signingIdentifier,
  verifySignature,
} = require("./lib/macos-signing");

async function signMacosRelease(options) {
  if (process.platform !== "darwin") throw new Error("Whisper release signing requires macOS.");
  const credentials = loadSigningCredentials();
  const fingerprint = certificateFingerprint();
  const password = randomBytes(32).toString("base64");
  const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "whisper-signing-")));
  const keychain = path.join(directory, "signing.keychain-db");
  let keychainCreated = false;
  function removeFromSearchList() {
    const searchList = run(
      "/usr/bin/security",
      ["list-keychains", "-d", "user"],
      "Reading keychain search list"
    );
    const current = [...searchList.matchAll(/"([^"\n]+)"/g)].map((match) => match[1]);
    if (current.includes(keychain)) {
      run(
        "/usr/bin/security",
        ["list-keychains", "-d", "user", "-s", ...current.filter((entry) => entry !== keychain)],
        "Removing temporary signing keychain from search list"
      );
    }
  }
  try {
    const certificate = path.join(directory, "identity.p12");
    fs.writeFileSync(certificate, credentials.certificate, { mode: 0o600 });
    run(
      "/usr/bin/security",
      ["create-keychain", "-p", password, keychain],
      "Creating temporary signing keychain"
    );
    keychainCreated = true;
    // Some macOS versions add new keychains to the search list automatically.
    removeFromSearchList();
    run(
      "/usr/bin/security",
      ["unlock-keychain", "-p", password, keychain],
      "Unlocking signing keychain"
    );
    run(
      "/usr/bin/security",
      ["set-keychain-settings", "-lut", "3600", keychain],
      "Configuring temporary signing keychain timeout"
    );
    run(
      "/usr/bin/security",
      [
        "import",
        certificate,
        "-k",
        keychain,
        "-P",
        credentials.password,
        "-T",
        "/usr/bin/codesign",
      ],
      "Importing signing identity"
    );
    run(
      "/usr/bin/security",
      ["set-key-partition-list", "-S", "apple-tool:,apple:", "-s", "-k", password, keychain],
      "Configuring signing key access"
    );
    const signedPaths = new Set();
    const { signAsync } = require("@electron/osx-sign");
    await signAsync({
      ...options,
      identity: fingerprint,
      keychain,
      identityValidation: false,
      preAutoEntitlements: false,
      // Resources are sealed by their bundle; only Mach-O code needs its own signature.
      ignore: createSigningIgnore(options.ignore),
      optionsForFile(file) {
        const inherited = options.optionsForFile ? options.optionsForFile(file) : {};
        const identifier = signingIdentifier(options.app, file);
        signedPaths.add(fs.realpathSync(file));
        return {
          ...inherited,
          timestamp: "none",
          requirements: `=designated => ${designatedRequirement(identifier, fingerprint)}`,
          additionalArguments: [
            ...(inherited.additionalArguments || []),
            "--identifier",
            identifier,
          ],
        };
      },
    });
    verifySignature(options.app, [...signedPaths]);
    console.log("Verified Whisper release signatures against the pinned certificate.");
  } finally {
    try {
      if (keychainCreated || fs.existsSync(keychain))
        run(
          "/usr/bin/security",
          ["delete-keychain", keychain],
          "Deleting temporary signing keychain"
        );
    } finally {
      try {
        removeFromSearchList();
      } finally {
        fs.rmSync(directory, { recursive: true, force: true });
      }
    }
  }
}

module.exports = signMacosRelease;

if (require.main === module) {
  try {
    if (process.argv[2] !== "--verify" || !process.argv[3]) {
      throw new Error(
        "Usage: node scripts/sign-macos-release.js --verify /path/to/Whisper.app [binary ...]"
      );
    }
    verifySignature(
      path.resolve(process.argv[3]),
      process.argv.slice(4).map((value) => path.resolve(value))
    );
    console.log("Verified Whisper release signatures against the pinned certificate.");
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
