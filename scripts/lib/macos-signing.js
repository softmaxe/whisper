const { spawnSync } = require("node:child_process");
const { createHash, X509Certificate } = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const APP_IDENTIFIER = "local.whisper.desktop";
const CERTIFICATE_PATH = path.resolve(__dirname, "../../resources/mac/signing-certificate.pem");
const SIGNING_DIRECTORY = path.join(os.homedir(), ".config/whisper/signing");

function run(command, args, description) {
  const result = spawnSync(command, args, { encoding: "utf8" });
  if (result.error || result.status !== 0) {
    // security import errors can include passwords in command arguments.
    throw new Error(`${description} failed${result.status === null ? "" : ` (${result.status})`}.`);
  }
  return result.stdout;
}

function loadSigningCredentials(env = process.env, directory = SIGNING_DIRECTORY) {
  const encoded = env.WHISPER_SIGNING_CERTIFICATE;
  const password = env.WHISPER_SIGNING_PASSWORD;
  if (encoded !== undefined || password !== undefined) {
    if (
      typeof encoded !== "string" ||
      !encoded.trim() ||
      typeof password !== "string" ||
      !password
    ) {
      throw new Error("Set both WHISPER_SIGNING_CERTIFICATE and WHISPER_SIGNING_PASSWORD.");
    }
    const normalized = encoded.replace(/\s/g, "");
    const certificate = Buffer.from(normalized, "base64");
    if (!certificate.length || certificate.toString("base64") !== normalized) {
      throw new Error("WHISPER_SIGNING_CERTIFICATE must contain a base64-encoded PKCS12 file.");
    }
    return { certificate, password };
  }
  try {
    const certificate = fs.readFileSync(path.join(directory, "identity.p12"));
    const password = fs.readFileSync(path.join(directory, "password"), "utf8").trimEnd();
    if (!certificate.length || !password) throw new Error("Empty signing credentials");
    return { certificate, password };
  } catch {
    throw new Error(
      "Signing identity is missing. Restore the existing Whisper signing backup or configure the release signing secrets."
    );
  }
}

function certificateFingerprint(certificatePath = CERTIFICATE_PATH) {
  try {
    return new X509Certificate(fs.readFileSync(certificatePath)).fingerprint.replaceAll(":", "");
  } catch {
    throw new Error("The pinned Whisper signing certificate is missing or invalid.");
  }
}

function designatedRequirement(identifier, fingerprint) {
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(identifier)) {
    throw new Error("Invalid code signing identifier.");
  }
  if (!/^[A-Fa-f0-9]{40}$/.test(fingerprint)) {
    throw new Error("Invalid signing certificate fingerprint.");
  }
  return `identifier "${identifier}" and certificate leaf = H"${fingerprint.toUpperCase()}"`;
}

function isSignableCode(filePath) {
  const stat = fs.statSync(filePath);
  if (stat.isDirectory()) return /\.(app|framework)$/.test(filePath);
  if (!stat.isFile() || stat.size < 4) return false;
  const descriptor = fs.openSync(filePath, "r");
  try {
    const header = Buffer.alloc(12);
    const length = fs.readSync(descriptor, header, 0, header.length, 0);
    const magic = header.readUInt32BE(0);
    if ([0xfeedface, 0xfeedfacf, 0xcefaedfe, 0xcffaedfe].includes(magic)) return true;
    if (length < 12) return false;
    const fat = [0xcafebabe, 0xcafebabf].includes(magic);
    const swappedFat = [0xbebafeca, 0xbfbafeca].includes(magic);
    if (!fat && !swappedFat) return false;
    const count = swappedFat ? header.readUInt32LE(4) : header.readUInt32BE(4);
    const cpu = swappedFat ? header.readUInt32LE(8) : header.readUInt32BE(8);
    // Java class files share FAT_MAGIC, but do not contain a Mach-O CPU header.
    return count > 0 && count <= 32 && [7, 12, 0x01000007, 0x0100000c].includes(cpu);
  } finally {
    fs.closeSync(descriptor);
  }
}

function createSigningIgnore(inheritedIgnore) {
  const inherited = Array.isArray(inheritedIgnore)
    ? inheritedIgnore
    : inheritedIgnore
      ? [inheritedIgnore]
      : [];
  const seen = new Set();
  return (file) => {
    if (
      inherited.some((ignore) => (typeof ignore === "function" ? ignore(file) : file.match(ignore)))
    )
      return true;
    if (!isSignableCode(file)) return true;
    const canonical = fs.realpathSync(file);
    if (seen.has(canonical)) return true;
    seen.add(canonical);
    return false;
  };
}

function canonicalPath(filePath) {
  try {
    return fs.realpathSync(filePath);
  } catch (error) {
    if (error.code === "ENOENT") return path.resolve(filePath);
    throw error;
  }
}

function plistValue(bundle, name) {
  for (const relative of ["Contents/Info.plist", "Resources/Info.plist", "Info.plist"]) {
    const plist = path.join(bundle, relative);
    if (fs.existsSync(plist)) {
      return run(
        "/usr/libexec/PlistBuddy",
        ["-c", `Print :${name}`, plist],
        `Reading ${name}`
      ).trim();
    }
  }
  throw new Error(`Missing Info.plist for signing bundle: ${bundle}`);
}

function signingIdentifier(appPath, filePath, readPlist = plistValue) {
  const app = canonicalPath(appPath);
  const file = canonicalPath(filePath);
  const relative = path.relative(app, file);
  if (relative.startsWith(`..${path.sep}`) || relative === ".." || path.isAbsolute(relative)) {
    throw new Error("Signed components must be inside the application bundle.");
  }
  if (file === app) {
    if (readPlist(app, "CFBundleIdentifier") !== APP_IDENTIFIER) {
      throw new Error(`Whisper must keep CFBundleIdentifier ${APP_IDENTIFIER}.`);
    }
    return APP_IDENTIFIER;
  }
  if (/\.(app|framework)$/.test(file)) return readPlist(file, "CFBundleIdentifier");

  // Signing a bundle overwrites its executable's signature. Give both the same ID.
  for (
    let parent = path.dirname(file);
    parent.startsWith(`${app}${path.sep}`) || parent === app;
    parent = path.dirname(parent)
  ) {
    if (/\.(app|framework)$/.test(parent)) {
      if (path.basename(file) === readPlist(parent, "CFBundleExecutable")) {
        return parent === app
          ? signingIdentifier(app, app, readPlist)
          : readPlist(parent, "CFBundleIdentifier");
      }
      break;
    }
  }
  const stablePath = relative.split(path.sep).join("/");
  const suffix = createHash("sha256").update(stablePath).digest("hex").slice(0, 24);
  return `${APP_IDENTIFIER}.binary.${suffix}`;
}

function verifySignature(appPath, binaries = [], certificatePath = CERTIFICATE_PATH) {
  const fingerprint = certificateFingerprint(certificatePath);
  for (const target of new Set([appPath, ...binaries])) {
    const identifier = signingIdentifier(appPath, target);
    const requirement = designatedRequirement(identifier, fingerprint);
    run(
      "/usr/bin/codesign",
      ["--verify", "--deep", "--strict", `-R=${requirement}`, target],
      `Verifying signature for ${path.basename(target)}`
    );
    const result = spawnSync("/usr/bin/codesign", ["--display", "--requirements", "-", target], {
      encoding: "utf8",
    });
    const actual = result.stdout.replace(
      /H"([a-f0-9]+)"/gi,
      (_match, hash) => `H"${hash.toUpperCase()}"`
    );
    if (
      result.status !== 0 ||
      actual.match(/^designated => (.+)$/m)?.[1] !== requirement ||
      /\bcdhash\b/.test(actual)
    ) {
      throw new Error(`Unstable designated requirement for ${path.basename(target)}.`);
    }
  }
}

module.exports = {
  APP_IDENTIFIER,
  CERTIFICATE_PATH,
  SIGNING_DIRECTORY,
  certificateFingerprint,
  createSigningIgnore,
  designatedRequirement,
  isSignableCode,
  loadSigningCredentials,
  run,
  signingIdentifier,
  verifySignature,
};
