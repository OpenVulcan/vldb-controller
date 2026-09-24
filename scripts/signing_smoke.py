"""Exercise release signing on disposable runners; never log credential material.
在一次性 Runner 上测试发行签名，绝不记录凭据内容。
"""

import base64
import hashlib
import hmac
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile
import time


def totp(seed: str, timestamp: int, digits: int = 6) -> str:
    """Return the SimplySign SHA-256 TOTP for a raw Base32 seed and Unix time.
    根据原始 Base32 种子和 Unix 时间返回 SimplySign 使用的 SHA-256 动态码。
    """
    canonical = seed.strip().upper().rstrip("=")
    if not re.fullmatch(r"[A-Z2-7]{16,128}", canonical):
        raise ValueError("totp_seed_must_be_raw_base32")
    key = base64.b32decode(canonical + "=" * (-len(canonical) % 8))
    digest = hmac.new(key, struct.pack(">Q", timestamp // 30), hashlib.sha256).digest()
    offset = digest[-1] & 15
    number = struct.unpack(">I", digest[offset:offset + 4])[0] & 0x7FFFFFFF
    return str(number % (10 ** digits)).zfill(digits)


def self_test() -> None:
    """Check RFC 6238 SHA-256 vectors against the algorithm used by CodeSignAuto.
    校验 RFC 6238 SHA-256 向量，与 CodeSignAuto 实际使用的算法保持一致。
    """
    seed = base64.b32encode(b"12345678901234567890123456789012").decode("ascii")
    vectors = [(59, "46119246"), (1111111109, "68084774"),
               (1111111111, "67062674"), (1234567890, "91819424"),
               (2000000000, "90698825"), (20000000000, "77737706")]
    for timestamp, expected in vectors:
        if totp(seed, timestamp, 8) != expected:
            raise RuntimeError("rfc6238_vector_failed")
    for invalid in ["", "not-a-seed", "0123456789", "otpauth://totp/test"]:
        try:
            totp(invalid, 59)
        except ValueError:
            continue
        raise RuntimeError("invalid_seed_accepted")
    print("RFC 6238 vectors and invalid-input checks passed")


def gpg_call(home: Path, arguments: list[str], stdin: bytes | None = None) -> subprocess.CompletedProcess:
    """Run GnuPG with a private keyring and capture all potentially identifying output.
    使用隔离密钥环运行 GnuPG，捕获所有可能包含身份信息的输出。
    """
    return subprocess.run(
        ["gpg", "--homedir", str(home), "--batch", "--no-tty", *arguments],
        input=stdin, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60, check=False,
    )


def require_success(result: subprocess.CompletedProcess, stage: str) -> None:
    """Raise a bounded diagnostic without exposing GnuPG output or key identities.
    返回有限诊断，不泄露 GnuPG 原始输出或密钥身份信息。
    """
    if result.returncode != 0:
        raise RuntimeError(f"{stage}_failed_exit_{result.returncode}")


def test_gpg(binary: Path, output: Path) -> None:
    """Sign an ELF fixture, verify with only its public key, and reject a modified copy.
    签名 ELF 测试程序，仅使用公钥验签，并拒绝被修改的副本。
    """
    output.mkdir(parents=True, exist_ok=True)
    report = {"kind": "gpg", "status": "failed", "stage": "credentials"}
    # Remove credentials from the environment inherited by native subprocesses.
    # 从原生子进程继承的环境中移除凭据。
    private_key = os.environ.pop("GPG_PRIVATE_KEY", "")
    passphrase = os.environ.pop("GPG_PASSPHRASE", "")
    try:
        if not private_key:
            raise RuntimeError("GPG_PRIVATE_KEY_unavailable_to_this_repository")
        if not private_key.lstrip().startswith("-----BEGIN PGP PRIVATE KEY BLOCK-----"):
            raise RuntimeError("GPG_PRIVATE_KEY_must_contain_an_armored_private_key")
        if binary.read_bytes()[:4] != b"\x7fELF":
            raise RuntimeError("fixture_is_not_elf")
        with tempfile.TemporaryDirectory(prefix="signing-gpg-") as temporary:
            signer = Path(temporary) / "signer"
            verifier = Path(temporary) / "verifier"
            signer.mkdir(mode=0o700)
            verifier.mkdir(mode=0o700)
            try:
                report["stage"] = "import"
                require_success(gpg_call(signer, ["--import"], private_key.encode()), "gpg_import")
                private_key = ""
                listing = gpg_call(signer, ["--with-colons", "--fixed-list-mode", "--list-secret-keys"])
                require_success(listing, "gpg_list")
                fingerprints = []
                pending = False
                for line in listing.stdout.decode("utf-8").splitlines():
                    fields = line.split(":")
                    if fields[0] == "sec":
                        if fields[1] in {"r", "e", "d"} or "s" not in fields[11].lower():
                            raise RuntimeError("gpg_key_is_not_usable_for_signing")
                        pending = True
                    elif pending and fields[0] == "fpr":
                        fingerprints.append(fields[9])
                        pending = False
                if len(fingerprints) != 1:
                    raise RuntimeError("exactly_one_gpg_primary_secret_key_required")
                fingerprint = fingerprints[0]
                report["primary_fingerprint"] = fingerprint
                report["stage"] = "sign"
                signature = output / "probe.asc"
                result = gpg_call(signer, [
                    "--pinentry-mode", "loopback", "--passphrase-fd", "0", "--local-user", fingerprint,
                    "--armor", "--detach-sign", "--output", str(signature), str(binary),
                ], (passphrase + "\n").encode())
                if result.returncode:
                    raise RuntimeError("gpg_sign_failed_check_GPG_PASSPHRASE_if_key_is_encrypted")
                public = gpg_call(signer, ["--armor", "--export", fingerprint])
                require_success(public, "gpg_export_public")
                (output / "public-key.asc").write_bytes(public.stdout)
                require_success(gpg_call(verifier, ["--import"], public.stdout), "gpg_import_public")
                report["stage"] = "verify"
                verified = gpg_call(verifier, ["--status-fd", "1", "--verify", str(signature), str(binary)])
                require_success(verified, "gpg_verify")
                valid = [line.split() for line in verified.stdout.decode().splitlines()
                         if line.startswith("[GNUPG:] VALIDSIG ")]
                if len(valid) != 1 or fingerprint not in {valid[0][2], valid[0][-1]}:
                    raise RuntimeError("gpg_verified_signer_mismatch")
                report["signing_fingerprint"] = valid[0][2]
                report["stage"] = "tamper_rejection"
                tampered = Path(temporary) / "tampered-probe"
                content = bytearray(binary.read_bytes())
                content[-1] ^= 1
                tampered.write_bytes(content)
                rejected = gpg_call(verifier, ["--status-fd", "1", "--verify", str(signature), str(tampered)])
                if rejected.returncode == 0 or b"[GNUPG:] BADSIG " not in rejected.stdout:
                    raise RuntimeError("gpg_tampered_fixture_not_rejected")
                report.update(status="passed", stage="complete", tamper_rejected=True,
                              sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
            finally:
                # Stop isolated agents before deleting the directories holding secret key material.
                # 删除包含私钥的隔离目录前，先停止对应的代理进程。
                for home in (signer, verifier):
                    subprocess.run(["gpgconf", "--homedir", str(home), "--kill", "all"],
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10, check=False)
    except Exception as error:
        report["error"] = str(error) if isinstance(error, RuntimeError) else type(error).__name__
        raise
    finally:
        (output / "gpg-result.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(json.dumps(report))


def tamper_pe(source: Path, destination: Path) -> None:
    """Modify a PE section byte covered by Authenticode, avoiding excluded header fields.
    修改 Authenticode 覆盖的 PE 节数据，避开不参与签名的头部字段。
    """
    content = bytearray(source.read_bytes())
    if content[:2] != b"MZ" or len(content) < 64:
        raise RuntimeError("invalid_pe_fixture")
    pe = struct.unpack_from("<I", content, 0x3C)[0]
    if content[pe:pe + 4] != b"PE\0\0":
        raise RuntimeError("invalid_pe_signature")
    optional_size = struct.unpack_from("<H", content, pe + 20)[0]
    section = pe + 24 + optional_size
    size, offset = struct.unpack_from("<II", content, section + 16)
    if not size or offset < section + 40 or offset + size > len(content):
        raise RuntimeError("invalid_pe_section")
    content[offset] ^= 1
    destination.write_bytes(content)


def main() -> None:
    """Dispatch explicit test modes; TOTP output is captured and masked by the caller.
    分派明确的测试模式；动态码输出由调用方捕获并脱敏。
    """
    mode = sys.argv[1]
    if mode == "self-test":
        self_test()
    elif mode == "totp":
        seed = os.environ.pop("CERTUM_TOTP_SECRET", "")
        remaining = 30 - int(time.time()) % 30
        if remaining < 10:
            time.sleep(remaining + 1)
        print(totp(seed, int(time.time())))
    elif mode == "gpg":
        test_gpg(Path(sys.argv[2]).resolve(), Path(sys.argv[3]).resolve())
    elif mode == "tamper-pe":
        tamper_pe(Path(sys.argv[2]), Path(sys.argv[3]))
    else:
        raise RuntimeError("unknown_signing_test_mode")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        # Never emit exception arguments: native failures can contain credential material.
        # 不输出异常参数：原生工具失败信息可能包含凭据内容。
        print(f"signing_smoke_failed: {type(error).__name__}", file=sys.stderr)
        sys.exit(1)
