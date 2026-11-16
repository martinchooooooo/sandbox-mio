# Source this file. Private spike environment: nothing here touches the user's ~/.ssh or the app's Mutagen daemon.
# SCRATCH must be a Windows-style path (forward slashes) to the scratch directory holding keys/, ssh_config, sshbin/, mdata/.
export MSYS_NO_PATHCONV=1
export SCRATCH="${SCRATCH:-C:/Users/krist/AppData/Local/Temp/claude/98e062e43dc2c464/0e25ff6b-7082-4f2f-badf-fbfa495ebb2a/scratchpad}"
SCRATCH_WIN="$(echo "$SCRATCH" | tr / "\\")"
export PODMAN_CONNECTION=podman-machine-default
export PODMAN_EXE="C:/Users/krist/prog/mi-sandbox/src-tauri/binaries/podman/podman.exe"
export MUTAGEN_EXE="C:/Users/krist/prog/mi-sandbox/src-tauri/binaries/mutagen-x86_64-pc-windows-msvc.exe"
export MUTAGEN_DATA_DIRECTORY="$SCRATCH_WIN\mdata"
export MUTAGEN_SSH_PATH="$SCRATCH_WIN\sshbin"
export SPIKE_SSH_CONFIG="$SCRATCH_WIN\ssh_config"
export SSH_EXE="C:/Windows/System32/OpenSSH/ssh.exe"
