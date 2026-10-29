#!/usr/bin/env bash
# 在东京服务器上把一个已发布 GitHub Release 的资产逐个中转到 123 云盘（pan123-upload.yml 经 SSH 以
# `bash -s` 执行，凭据由 stdin 前置的 export 传入，不进进程参数）。
#
# GitHub runner 直传 123 WebDAV 极慢（20 分钟传不完 45MB）；服务器下 GitHub 快、传 123 约 0.8MB/s。
# 逐个文件：下载到临时目录 → PUT → PROPFIND 核对大小 → 立即删除，磁盘占用不超过一个资产。
# 远端已有同名且大小一致的文件跳过，重复执行幂等（补发组件后再跑只传新增 / 变化的文件）。
# 临时目录在任何退出（含 SSH 断开的 HUP）时由 trap 删除；启动时顺带清掉 6 小时前遗留的目录。
#
# 目录与 OSS 同一契约（website-v2/src/lib/oss.ts 的 releaseObjectKey，改一处须同步另一处）：
#   <WebDAV 授权目录>/FluxDownRelease/<版本>/<组件>/<asset-name>
#
# 环境：DAV_USER / DAV_PASSWORD（必填）、TAG、REPO（owner/repo）、PATTERN（可选 shell 通配）、
# DRY_RUN（true = 只列出将上传的文件）、DAV_URL / PREFIX（有默认值）。依赖 curl、jq、flock。
set -euo pipefail

: "${DAV_USER:?}" "${DAV_PASSWORD:?}" "${TAG:?}" "${REPO:?}"
DAV_URL=${DAV_URL:-https://webdav.123pan.cn/webdav}
PREFIX=${PREFIX:-FluxDownRelease}
PATTERN=${PATTERN:-}
DRY_RUN=${DRY_RUN:-false}

case "$TAG" in *[!A-Za-z0-9._-]*|"") echo "::error::invalid tag '$TAG'"; exit 1 ;; esac
case "$REPO" in */*) ;; *) echo "::error::invalid repo '$REPO'"; exit 1 ;; esac
case "$TAG" in
  v*)   COMPONENT=app; VERSION=$TAG ;;
  *-v*) COMPONENT=${TAG%%-v*}; VERSION=v${TAG#*-v} ;;
  *)    echo "::error::invalid tag '$TAG'"; exit 1 ;;
esac
BASE=${DAV_URL%/}
REL="$PREFIX/$VERSION/$COMPONENT"

# 同一时间只跑一个中转，避免并发下载占满磁盘
exec 9>/tmp/pan123-mirror.lock
flock 9
find /tmp -maxdepth 1 -name 'pan123-mirror.*' -type d -mmin +360 -exec rm -rf {} +

WORK=$(mktemp -d /tmp/pan123-mirror.XXXXXX)
WATCHDOG=""
cleanup() {
  if [ -n "$WATCHDOG" ]; then kill -KILL "$WATCHDOG" 2>/dev/null || true; fi
  rm -rf "$WORK"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 141' PIPE

# 无 pty 的 SSH 会话断开（runner 取消 / 网络中断）时 sshd 不发 HUP，bash 被过继给 init 继续跑：
# 监视启动时的父进程，父进程消失就向本进程组发 TERM（含命令替换里的 curl），本进程走 TERM → EXIT 清理。
SESSION_PARENT=$PPID
if [ "$SESSION_PARENT" -gt 1 ]; then
  SELF=$$
  (
    trap '' TERM
    while kill -0 "$SESSION_PARENT" 2>/dev/null; do sleep 5; done
    PGID=$(ps -o pgid= -p "$SELF" | tr -d ' ')
    if [ "$PGID" = "$SELF" ]; then kill -TERM -- "-$PGID" 2>/dev/null || true
    else pkill -TERM -g "$PGID" -x curl || true; kill -TERM "$SELF" 2>/dev/null || true
    fi
  ) </dev/null >/dev/null 2>&1 &
  WATCHDOG=$!
fi

NETRC="$WORK/netrc"
HOST=${BASE#*://}; HOST=${HOST%%/*}
( umask 077; printf 'machine %s login %s password %s\n' "$HOST" "$DAV_USER" "$DAV_PASSWORD" > "$NETRC" )
dav() { curl -sS --netrc-file "$NETRC" --connect-timeout 20 "$@"; }
remote_size() {
  dav -m 60 -X PROPFIND -H 'Depth: 0' "$1" \
    | grep -o '<[A-Za-z:]*getcontentlength>[0-9]*' | sed 's/.*>//' || true
}

ROOT_CODE=$(dav -m 60 -o /dev/null -w '%{http_code}' -X PROPFIND -H 'Depth: 0' "$BASE/") || ROOT_CODE=000
if [ "$ROOT_CODE" != 207 ]; then
  echo "::error::123pan WebDAV PROPFIND $BASE/ returned HTTP $ROOT_CODE（账号 / 应用密码 / 授权目录有误？）"
  exit 1
fi

# 已发布 release 的资产清单（草稿不可见：只中转对外可下载的版本）
curl -sSf -m 60 -H 'Accept: application/vnd.github+json' \
  "https://api.github.com/repos/$REPO/releases/tags/$TAG" > "$WORK/release.json"
jq -r '.assets[] | "\(.size)\t\(.name)\t\(.browser_download_url)"' "$WORK/release.json" > "$WORK/assets.tsv"

DIR_PATH=""
IFS=/ read -ra SEGMENTS <<< "$REL"
for seg in "${SEGMENTS[@]}"; do
  DIR_PATH="$DIR_PATH/$(printf '%s' "$seg" | jq -sRr @uri)"
  if [ "$DRY_RUN" = "false" ]; then
    # PUT 不会自动建父目录；已存在时 123 返回 201，标准 WebDAV 返回 405
    CODE=$(dav -m 60 -o /dev/null -w '%{http_code}' -X MKCOL "$BASE$DIR_PATH/") || CODE=000
    case "$CODE" in
      201|405) ;;
      *) echo "::error::MKCOL $DIR_PATH/ returned HTTP $CODE"; exit 1 ;;
    esac
  fi
done

echo "Mirroring $TAG → 123pan:/${REL}/"
UPLOADED=0 SKIPPED=0 FAILED=0
while IFS=$'\t' read -r size name url; do
  # shellcheck disable=SC2254
  if [ -n "$PATTERN" ]; then case "$name" in $PATTERN) ;; *) continue ;; esac; fi
  target="$BASE$DIR_PATH/$(printf '%s' "$name" | jq -sRr @uri)"
  if [ "$(remote_size "$target")" = "$size" ]; then
    SKIPPED=$((SKIPPED + 1)); echo "= $name (already present)"; continue
  fi
  if [ "$DRY_RUN" != "false" ]; then echo "would upload $name ($size bytes)"; continue; fi
  file="$WORK/asset"
  ok=false
  for i in 1 2 3; do
    if curl -sSfL --connect-timeout 20 --retry 3 -o "$file" "$url" \
       && [ "$(stat -c %s "$file")" = "$size" ]; then
      CODE=$(dav -X PUT -T "$file" --speed-limit 10240 --speed-time 120 \
        -o /dev/null -w '%{http_code}' "$target") || CODE=000
      if [ "$(remote_size "$target")" = "$size" ]; then ok=true; break; fi
      echo "⚠️  $name: upload not verified, PUT HTTP $CODE (attempt $i)"
    else
      echo "⚠️  $name: GitHub download failed or size mismatch (attempt $i)"
    fi
    [ "$i" = 3 ] || sleep 15
  done
  rm -f "$file"
  if $ok; then
    UPLOADED=$((UPLOADED + 1)); echo "✓ $name ($size bytes)"
  else
    FAILED=$((FAILED + 1)); echo "::error::123pan upload failed after 3 attempts: $name"
  fi
done < "$WORK/assets.tsv"

echo "uploaded=$UPLOADED skipped=$SKIPPED failed=$FAILED"
[ "$FAILED" -eq 0 ]
