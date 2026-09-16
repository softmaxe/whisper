// One-time setup. Release builds must reuse this identity, never generate a new one.
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { randomBytes, X509Certificate } = require("node:crypto");
const { spawnSync } = require("node:child_process");

const backup = path.join(os.homedir(), ".config", "whisper", "signing");
const publicCertificate = path.join(__dirname, "..", "resources", "mac", "signing-certificate.pem");

function openssl(args, password) {
  const result = spawnSync("/usr/bin/openssl", args, {
    env: { ...process.env, WHISPER_P12_PASSWORD: password },
    stdio: "pipe",
  });
  if (result.error || result.status !== 0) {
    throw new Error(
      "OpenSSL could not create the signing identity; no existing identity was changed."
    );
  }
}

function main() {
  if (process.platform !== "darwin") throw new Error("Create the signing identity on macOS.");
  if (fs.existsSync(publicCertificate) || fs.existsSync(backup)) {
    throw new Error(
      "A signing identity already exists. Restore its backup instead of generating a replacement."
    );
  }

  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "whisper-signing-create-"));
  const key = path.join(temporary, "private.key");
  const certificate = path.join(temporary, "certificate.pem");
  const config = path.join(temporary, "openssl.cnf");
  const p12 = path.join(temporary, "identity.p12");
  const password = randomBytes(32).toString("base64url");
  const oldUmask = process.umask(0o077);
  let createdBackup = false;
  try {
    fs.writeFileSync(
      config,
      "[req]\nprompt = no\ndistinguished_name = subject\nx509_extensions = signing\n" +
        "[subject]\nCN = Whisper Release Signing\n" +
        "[signing]\nbasicConstraints = critical,CA:false\n" +
        "keyUsage = critical,digitalSignature\nextendedKeyUsage = critical,codeSigning\n"
    );
    openssl(
      [
        "req",
        "-x509",
        "-newkey",
        "rsa:3072",
        "-nodes",
        "-sha256",
        "-days",
        "7300",
        "-config",
        config,
        "-keyout",
        key,
        "-out",
        certificate,
      ],
      password
    );
    openssl(
      [
        "pkcs12",
        "-export",
        "-inkey",
        key,
        "-in",
        certificate,
        "-out",
        p12,
        "-name",
        "Whisper Release Signing",
        "-passout",
        "env:WHISPER_P12_PASSWORD",
      ],
      password
    );
    const pem = fs.readFileSync(certificate);
    const parsed = new X509Certificate(pem);
    fs.mkdirSync(path.dirname(backup), { recursive: true, mode: 0o700 });
    fs.mkdirSync(backup, { mode: 0o700 });
    createdBackup = true;
    fs.copyFileSync(p12, path.join(backup, "identity.p12"), fs.constants.COPYFILE_EXCL);
    fs.writeFileSync(path.join(backup, "password"), password, { flag: "wx", mode: 0o600 });
    fs.writeFileSync(publicCertificate, pem, { flag: "wx", mode: 0o644 });
    console.log(`Created encrypted signing backup at ${backup}`);
    console.log(`Public certificate SHA-256: ${parsed.fingerprint256}`);
    console.log("Back up identity.p12 and password securely. Every release must reuse them.");
  } catch (error) {
    if (createdBackup && !fs.existsSync(publicCertificate)) {
      fs.rmSync(backup, { recursive: true, force: true });
    }
    throw error;
  } finally {
    process.umask(oldUmask);
    fs.rmSync(temporary, { recursive: true, force: true });
  }
}

try {
  main();
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
