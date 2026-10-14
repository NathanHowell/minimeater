"""Where OpenFOAM runs: the opencfd image under whichever container runtime the machine has (Apple's `container` on the
Mac, podman or docker on the Linux box). `MINIMEATER_CONTAINER` picks one by name."""

import os
import shutil
import subprocess
from pathlib import Path

IMAGE = "docker.io/opencfd/openfoam-run:2606"
FOAM = "/usr/lib/openfoam/openfoam2606/etc/bashrc"
RUNTIMES = ("container", "podman", "docker")


def runtime() -> str:
    want = os.environ.get("MINIMEATER_CONTAINER")
    for r in (want,) if want else RUNTIMES:
        if r and shutil.which(r):
            return r
    raise SystemExit(f"no container runtime ({', '.join(RUNTIMES)}) on PATH")


def command(case_dir: Path, script: str, cpus: int, mem: str) -> list[str]:
    """The command line that runs `script` (a path inside the container; the case is mounted at /case) with the OpenFOAM
    environment, on `cpus` cores and `mem` of memory."""
    rt = runtime()
    limits = ["-c", str(cpus), "-m", mem] if rt == "container" else ["--cpus", str(cpus), "--memory", mem]
    return [rt, "run", "--rm", *limits, "-v", f"{case_dir}:/case", IMAGE, "bash", "-c", f"source {FOAM} && {script}"]


def run(case_dir: Path, script: str, cpus: int, mem: str) -> int:
    return subprocess.run(command(case_dir, script, cpus, mem)).returncode
