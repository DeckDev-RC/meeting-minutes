import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

const repo = process.cwd();
const tmpDir = join(process.env.TEMP, "meeting-minutes-diarize-runtime-pack-test");
const projectRoot = join(tmpDir, "project");
const outDir = join(tmpDir, "out");
const extractDir = join(tmpDir, "extract");
const installRoot = join(tmpDir, "installed");
const outsideTarget = join(tmpDir, "outside-target");
const resourceDir = join(tmpDir, "resources", "diarize");
const pythonHome = join(tmpDir, "python-home");

const powershell = process.env.POWERSHELL_EXE || "powershell";

const runPowerShell = (args) => {
  const result = spawnSync(
    powershell,
    ["-NoProfile", "-ExecutionPolicy", "Bypass", ...args],
    {
      cwd: repo,
      encoding: "utf8",
    },
  );
  assert.equal(
    result.status,
    0,
    `PowerShell failed\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
  );
  return result;
};

rmSync(tmpDir, { recursive: true, force: true });
mkdirSync(join(projectRoot, ".venv-diarize", "Scripts"), { recursive: true });
mkdirSync(join(projectRoot, ".venv-diarize", "Lib", "site-packages"), { recursive: true });
mkdirSync(join(projectRoot, "scripts"), { recursive: true });
mkdirSync(outDir, { recursive: true });
mkdirSync(join(pythonHome, "Lib"), { recursive: true });
mkdirSync(join(pythonHome, "Lib", "site-packages", "global_pkg"), { recursive: true });
mkdirSync(join(pythonHome, "Scripts"), { recursive: true });

writeFileSync(join(projectRoot, ".venv-diarize", "Scripts", "python.exe"), "fake python");
writeFileSync(
  join(projectRoot, ".venv-diarize", "pyvenv.cfg"),
  `home = ${pythonHome}\nexecutable = ${join(pythonHome, "python.exe")}\n`,
);
writeFileSync(join(projectRoot, ".venv-diarize", "Lib", "site-packages", "diarize.pth"), "fake");
writeFileSync(join(pythonHome, "python.exe"), "fake base python");
writeFileSync(join(pythonHome, "python311.dll"), "fake base dll");
writeFileSync(join(pythonHome, "python311.pdb"), "do not copy");
writeFileSync(join(pythonHome, "python311_d.dll"), "do not copy");
writeFileSync(join(pythonHome, "Lib", "os.py"), "fake stdlib");
writeFileSync(join(pythonHome, "Lib", "site-packages", "global_pkg", "__init__.py"), "do not copy");
writeFileSync(join(pythonHome, "Scripts", "pip.exe"), "do not copy");
writeFileSync(
  join(projectRoot, "scripts", "diarize_cpu_backend.py"),
  "print('fake diarize backend')\n",
);
writeFileSync(
  join(projectRoot, "scripts", "requirements-diarize-cpu.txt"),
  "diarize==0.1.2\n",
);
writeFileSync(join(projectRoot, "scripts", "cloud_asr_benchmark.py"), "do not package\n");

runPowerShell([
  "-File",
  "scripts\\build_diarize_runtime_pack.ps1",
  "-ProjectRoot",
  projectRoot,
  "-OutDir",
  outDir,
  "-Version",
  "9.9.9-test",
]);

const zipFiles = readdirSync(outDir).filter((name) => name.endsWith(".zip"));
assert.equal(zipFiles.length, 1);
const packPath = join(outDir, zipFiles[0]);
assert.ok(zipFiles[0].includes("diarize-runtime"));

runPowerShell([
  "-Command",
  `Expand-Archive -LiteralPath '${packPath.replaceAll("'", "''")}' -DestinationPath '${extractDir.replaceAll("'", "''")}' -Force`,
]);

assert.ok(existsSync(join(extractDir, "runtime-manifest.json")));
assert.ok(existsSync(join(extractDir, ".python", "python.exe")));
assert.ok(existsSync(join(extractDir, ".python", "python311.dll")));
assert.equal(existsSync(join(extractDir, ".python", "python311.pdb")), false);
assert.equal(existsSync(join(extractDir, ".python", "python311_d.dll")), false);
assert.equal(existsSync(join(extractDir, ".python", "Lib", "site-packages")), false);
assert.equal(existsSync(join(extractDir, ".python", "Scripts")), false);
assert.ok(existsSync(join(extractDir, ".venv-diarize", "Scripts", "python.exe")));
assert.ok(existsSync(join(extractDir, ".venv-diarize", "Lib", "site-packages", "diarize.pth")));
assert.ok(existsSync(join(extractDir, "scripts", "diarize_cpu_backend.py")));
assert.ok(existsSync(join(extractDir, "scripts", "requirements-diarize-cpu.txt")));
assert.equal(existsSync(join(extractDir, "scripts", "cloud_asr_benchmark.py")), false);

const manifest = JSON.parse(readFileSync(join(extractDir, "runtime-manifest.json"), "utf8"));
assert.equal(manifest.name, "meeting-minutes-diarize-runtime");
assert.equal(manifest.version, "9.9.9-test");
assert.equal(manifest.layout.python, ".python/python.exe");
assert.equal(manifest.layout.sitePackages, ".venv-diarize/Lib/site-packages");
assert.equal(manifest.layout.backendScript, "scripts/diarize_cpu_backend.py");

mkdirSync(outsideTarget, { recursive: true });
writeFileSync(join(outsideTarget, "sentinel.txt"), "must survive");
mkdirSync(installRoot, { recursive: true });
runPowerShell([
  "-Command",
  `New-Item -ItemType Junction -Path '${join(installRoot, "stale-link").replaceAll("'", "''")}' -Target '${outsideTarget.replaceAll("'", "''")}' | Out-Null`,
]);

runPowerShell([
  "-File",
  "scripts\\install_diarize_runtime_pack.ps1",
  "-PackPath",
  packPath,
  "-InstallRoot",
  installRoot,
  "-NoEnv",
]);

assert.ok(existsSync(join(installRoot, ".venv-diarize", "Scripts", "python.exe")));
assert.ok(existsSync(join(installRoot, ".python", "python.exe")));
assert.ok(existsSync(join(installRoot, "scripts", "diarize_cpu_backend.py")));
assert.ok(existsSync(join(installRoot, "runtime-manifest.json")));
assert.ok(existsSync(join(installRoot, "installed-manifest.json")));
assert.equal(existsSync(join(installRoot, "scripts", "cloud_asr_benchmark.py")), false);
assert.ok(existsSync(join(outsideTarget, "sentinel.txt")));
assert.equal(existsSync(join(installRoot, "stale-link")), false);

const installed = JSON.parse(readFileSync(join(installRoot, "installed-manifest.json"), "utf8"));
assert.ok(existsSync(installed.installRoot));
assert.equal(
  readFileSync(join(installed.installRoot, "runtime-manifest.json"), "utf8"),
  readFileSync(join(installRoot, "runtime-manifest.json"), "utf8"),
);
assert.equal(installed.envConfigured, false);

runPowerShell([
  "-File",
  "scripts\\stage_diarize_runtime_resource.ps1",
  "-ProjectRoot",
  projectRoot,
  "-ResourceDir",
  resourceDir,
  "-Version",
  "9.9.9-test",
]);

assert.ok(existsSync(join(resourceDir, ".venv-diarize", "Scripts", "python.exe")));
assert.ok(existsSync(join(resourceDir, ".python", "python.exe")));
assert.ok(existsSync(join(resourceDir, "scripts", "diarize_cpu_backend.py")));
assert.ok(existsSync(join(resourceDir, "scripts", "requirements-diarize-cpu.txt")));
assert.ok(existsSync(join(resourceDir, "runtime-manifest.json")));
assert.equal(existsSync(join(resourceDir, "scripts", "cloud_asr_benchmark.py")), false);

const staged = JSON.parse(readFileSync(join(resourceDir, "runtime-manifest.json"), "utf8"));
assert.equal(staged.name, "meeting-minutes-diarize-runtime");
assert.equal(staged.version, "9.9.9-test");
assert.equal(staged.layout.python, ".python/python.exe");
assert.equal(staged.layout.sitePackages, ".venv-diarize/Lib/site-packages");
