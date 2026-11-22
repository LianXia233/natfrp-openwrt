#!/bin/sh
# 把仓库里的文件直接铺到设备（不打包，适合开发调试）
#
#   HOST=root@192.168.1.1 ./scripts/deploy.sh
#
# 认证方式二选一：
#   - SSH 密钥（推荐，直接 ssh 即可）
#   - SSH_ASKPASS 脚本（export SSH_ASKPASS=/path/to/askpass）
#
# 需要先跑过 ./scripts/build.sh 生成 src/natfrpctl/files/natfrpctl-<arch>。
set -eu

HOST=${HOST:-root@192.168.1.1}
ARCH=${ARCH:-aarch64}
root=$(cd "$(dirname "$0")/.." && pwd)
S="$root/src/luci-app-natfrp"
BIN="$root/src/natfrpctl/files/natfrpctl-$ARCH"

[ -f "$BIN" ] || { echo "缺少 $BIN，先运行 ./scripts/build.sh" >&2; exit 1; }

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

mkdir -p "$stage/usr/bin" \
         "$stage/usr/share/luci/menu.d" \
         "$stage/usr/share/rpcd/acl.d" \
         "$stage/www/luci-static/resources/view/natfrp" \
         "$stage/etc/config" "$stage/etc/init.d"

cp "$BIN"                                                  "$stage/usr/bin/natfrpctl"
cp "$S/root/usr/share/luci/menu.d/luci-app-natfrp.json"     "$stage/usr/share/luci/menu.d/"
cp "$S/root/usr/share/rpcd/acl.d/luci-app-natfrp.json"      "$stage/usr/share/rpcd/acl.d/"
cp "$S/htdocs/luci-static/resources/view/natfrp/main.js"    "$stage/www/luci-static/resources/view/natfrp/"
cp "$S/root/etc/config/natfrp"                             "$stage/etc/config/natfrp.default"
cp "$S/root/etc/init.d/natfrp"                             "$stage/etc/init.d/natfrp"

tar -cf - -C "$stage" . | \
ssh -o StrictHostKeyChecking=accept-new "$HOST" '
	set -e
	tar -xf - -C /

	# 若从旧版 Lua 前端迁移，顺手清掉遗留文件
	rm -f /usr/lib/lua/luci/controller/natfrp.lua \
	      /usr/lib/lua/luci/model/cbi/natfrp.lua
	rm -rf /usr/lib/lua/luci/view/natfrp

	chown root:root /usr/bin/natfrpctl \
		/usr/share/luci/menu.d/luci-app-natfrp.json \
		/usr/share/rpcd/acl.d/luci-app-natfrp.json \
		/etc/init.d/natfrp
	chown -R root:root /www/luci-static/resources/view/natfrp
	chmod 755 /usr/bin/natfrpctl /etc/init.d/natfrp
	chmod 644 /usr/share/luci/menu.d/luci-app-natfrp.json \
		/usr/share/rpcd/acl.d/luci-app-natfrp.json

	# 配置文件只在缺失时创建，不覆盖已有设置
	if [ ! -f /etc/config/natfrp ]; then
		mv /etc/config/natfrp.default /etc/config/natfrp
		chmod 600 /etc/config/natfrp
	else
		rm -f /etc/config/natfrp.default
	fi

	rm -f /tmp/luci-indexcache.*
	rm -rf /tmp/luci-modulecache
	/etc/init.d/rpcd restart 2>/dev/null || true
	/etc/init.d/natfrp restart >/dev/null 2>&1 || true

	sleep 2
	echo "部署完成：服务 > SakuraFrp 内网穿透"
	/usr/bin/natfrpctl status
'
