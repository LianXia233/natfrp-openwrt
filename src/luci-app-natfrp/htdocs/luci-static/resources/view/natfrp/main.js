'use strict';
'require view';
'require form';
'require fs';
'require uci';
'require poll';
'require ui';

/*
 * SakuraFrp 内网穿透 —— LuCI 页面
 *
 * 表单字段由 LuCI 的 uci 模块负责落盘；页面额外做两件事：
 *   1. 调用 /usr/bin/natfrpctl 读取运行状态，并展示 WebUI 一键入口；
 *   2. 「保存并应用」时调用 natfrpctl apply，把配置交给守护进程并让服务生效。
 */

var CTL = '/usr/bin/natfrpctl';
/* 只读入口：status 不含口令，因此只读账号也能安全轮询 */
var CTL_RO = '/usr/bin/natfrpctl-status';
var REMOTE_URL = 'https://www.natfrp.com/remote/v2';
/* 状态轮询间隔。status 会 fork pidof/uci，5 秒一轮在小内存设备上过于频繁 */
var POLL_INTERVAL = 15;

/*
 * hero 样式内联下发，避免额外请求与主题缓存问题。
 * 颜色全部走半透明灰阶，深色/浅色主题下都能自适应。
 */
var HERO_CSS = [
	'.nf-hero{border:1px solid rgba(127,127,127,.25);border-radius:12px;background:rgba(127,127,127,.06);',
		'padding:12px 14px;margin:0 0 1rem;display:flex;flex-direction:column;gap:8px}',
	'.nf-hero-body{display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap}',
	'.nf-hero-head{display:flex;align-items:center;gap:8px;flex-wrap:wrap;min-height:24px}',
	'.nf-dot{width:8px;height:8px;border-radius:50%;background:#b3261e;flex:0 0 auto}',
	'.nf-hero.is-run .nf-dot{background:#0a7d33;box-shadow:0 0 0 4px rgba(10,125,51,.14)}',
	'.nf-state{font-size:1.0625rem;font-weight:600;line-height:1.2}',
	'.nf-meta{font-size:.8125rem;opacity:.65}',
	'.nf-chip{font-size:.75rem;line-height:1.6;padding:0 8px;border-radius:999px;background:rgba(127,127,127,.15);white-space:nowrap}',
	'.nf-hero-actions{display:flex;gap:8px;flex-wrap:wrap}',
	/* 固定最小宽度，文案切换（复制 -> 已复制）时按钮不会伸缩，避免布局抖动 */
	'.nf-hero-actions>.nf-btn{min-width:7.5rem;text-align:center}',
	'.nf-btn.nf-ok{color:#0a7d33;border-color:rgba(10,125,51,.5)}',
	'.nf-btn.nf-bad{color:#b3261e;border-color:rgba(179,38,30,.5)}',
	'.nf-cert{padding:0 8px;font-size:.8125rem;line-height:1.5;min-height:0;height:auto;align-self:center}',
	'.nf-hero-foot{display:flex;align-items:center;gap:4px 10px;flex-wrap:wrap;font-size:.8125rem;opacity:.85}',
	'.nf-hero-foot .nf-hint{opacity:.8}',
	'.nf-hero code{background:rgba(127,127,127,.16);border-radius:6px;padding:1px 6px;font-size:.8125rem;word-break:break-all}',
	'@media(max-width:600px){',
		'.nf-hero{padding:10px 12px;gap:10px}',
		'.nf-hero-body{flex-direction:column;align-items:stretch;gap:10px}',
		'.nf-hero-actions{width:100%}',
		'.nf-hero-actions>.nf-btn{flex:1 1 0;text-align:center;padding-left:0;padding-right:0}',
		'.nf-hero-head{justify-content:flex-start}',
	'}'
].join('');

function parseJson(raw) {
	if (!raw)
		return null;
	try {
		return JSON.parse(raw);
	}
	catch (e) {
		return null;
	}
}

function readStatus() {
	return fs.exec(CTL_RO, [ 'status' ]).then(function (res) {
		return parseJson(res ? String(res.stdout || '').trim() : '');
	}).catch(function () {
		return null;
	});
}

/*
 * 口令单独取：status 不返回明文，只读会话拿不到 secrets，此时页面隐藏口令而不是报错。
 * 返回值：{ webui_pass, remote_pass }，无权限或失败时为 null。
 */
function readSecrets(attempt) {
	var n = attempt || 0;

	/* 首次打开页面时 rpcd 可能还没准备好，失败一次再重试，避免误显示“需要写权限” */
	var retry = function () {
		return (n < 1)
			? new Promise(function (r) { setTimeout(r, 700); }).then(function () { return readSecrets(n + 1); })
			: null;
	};

	return fs.exec(CTL, [ 'secrets' ]).then(function (res) {
		var data = parseJson(res ? String(res.stdout || '').trim() : '');
		if (data && data.ok === true)
			return data;
		return retry();
	}).catch(function () {
		return retry();
	});
}

function applySettings() {
	return fs.exec(CTL, [ 'apply' ]).then(function (res) {
		var data = parseJson(res ? String(res.stdout || '').trim() : '');
		if (!data || data.ok !== true)
			throw new Error((data && data.error) || _('守护进程没有接受这次配置'));
		return data;
	});
}

/*
 * 按钮上的瞬时反馈：直接改按钮文案，不走全局通知条。
 * 全局通知插在页面顶部，连点会叠出多条并把内容顶下去。
 */
function flashButton(btn, text, tone, ms) {
	if (!btn)
		return;

	if (btn.nfLabel == null)
		btn.nfLabel = btn.textContent;

	var label = btn.nfLabel;

	btn.textContent = text;
	btn.classList.remove('nf-ok', 'nf-bad');
	if (tone)
		btn.classList.add(tone);

	if (btn.nfTimer)
		clearTimeout(btn.nfTimer);

	btn.nfTimer = setTimeout(function () {
		btn.textContent = label;
		btn.classList.remove('nf-ok', 'nf-bad');
		btn.nfTimer = null;
	}, ms || 1600);
}

/*
 * LuCI 一般通过 HTTP 访问，navigator.clipboard 在非安全上下文里不可用，
 * 因此保留 execCommand 回退。
 */
function copyText(text) {
	if (navigator.clipboard && window.isSecureContext)
		return navigator.clipboard.writeText(text);

	return new Promise(function (resolve, reject) {
		var ta = E('textarea', { 'style': 'position:fixed;left:-9999px;top:0' }, [ text ]);
		document.body.appendChild(ta);
		ta.select();
		var ok = false;
		try {
			ok = document.execCommand('copy');
		}
		catch (e) {
			ok = false;
		}
		document.body.removeChild(ta);
		return ok ? resolve() : reject(new Error('copy failed'));
	});
}

return view.extend({
	load: function () {
		return Promise.all([ readStatus(), uci.load('natfrp') ]);
	},

	render: function (data) {
		var status = (data && data[0]) || null;
		var secrets = this.secrets || null;
		var m, s, o;

		this.badgeEl = null;
		this.webuiBtn = null;
		this.copyBtn = null;

		m = new form.Map('natfrp', _('SakuraFrp 内网穿透'),
			_('填写访问密钥后即可在服务端管理隧道；远程管理与 WebUI 需要额外设置密码。'));

		s = m.section(form.NamedSection, 'main', 'natfrp');
		s.anonymous = true;
		s.addremove = false;

		o = s.option(form.Flag, 'enabled', _('启用服务'),
			_('开机自启，并在保存后立即启动或停止守护进程。'));
		o.rmempty = false;
		o.default = '0';

		o = s.option(form.Value, 'token', _('访问密钥'),
			_('在服务端控制台的访问密钥页面获取。留空表示保持现有密钥不变。'));
		o.password = true;
		o.rmempty = true;
		o.placeholder = _('留空则不修改');

		o = s.option(form.Flag, 'remote_mgmt', _('启用远程管理'),
			_('允许在服务端网页远程操作路由器上的启动器。'));
		o.rmempty = false;
		o.default = '0';

		o = s.option(form.Value, 'remote_mgmt_pass', _('E2E 密码'),
			_('至少 8 个字符。留空则首次启用时自动生成随机密码，已有密码时保持不变。'));
		o.password = true;
		o.rmempty = true;
		o.depends('remote_mgmt', '1');
		o.placeholder = _('至少 8 个字符');
		o.validate = function (section_id, value) {
			if (!value)
				return true;
			return (value.length >= 8) || _('密码至少需要 8 个字符');
		};

		o = s.option(form.Flag, 'remote_allow_config', _('允许远程修改配置'),
			_('远程管理连接建立后，是否允许从服务端网页改写本机启动器配置。关掉更安全。'));
		o.rmempty = false;
		o.default = '1';
		o.depends('remote_mgmt', '1');

		o = s.option(form.Flag, 'remote_allow_update', _('允许远程触发更新'),
			_('远程管理连接建立后，是否允许从服务端网页触发本机启动器更新。关掉更安全。'));
		o.rmempty = false;
		o.default = '1';
		o.depends('remote_mgmt', '1');

		o = s.option(form.Flag, 'webui', _('启用 WebUI'),
			_('在本机启动启动器自带的图形界面，可独立于服务端使用。'));
		o.rmempty = false;
		o = s.option(form.Flag, 'webui_tls', _('使用 TLS（HTTPS）'),
			_('默认开启：自签名证书，可在下方「下载证书」导入信任。关闭后页面以明文 HTTP 提供，' +
				'口令与登录过程都不加密，仅建议在可信内网使用。'));
		o.rmempty = false;
		o.default = '1';
		o.depends('webui', '1');

		o = s.option(form.Value, 'webui_port', _('监听端口'));
		o.default = '4101';
		o.rmempty = true;
		o.depends('webui', '1');
		o.placeholder = '4101';
		o.validate = function (section_id, value) {
			if (!value)
				return true;
			if (!/^\d+$/.test(value))
				return _('端口必须是数字');
			var port = parseInt(value, 10);
			if (port < 1 || port > 65535)
				return _('端口范围是 1-65535');

			/* 关闭 TLS 时，明文入口与守护进程各占一个端口（后者的内部端口是 port+1000），
			   端口太小时偏移量无处可用，两者会抢同一端口 */
			var tls = uci.get('natfrp', 'main', 'webui_tls');
			if (tls === '0' && port <= 1000)
				return _('关闭 TLS 时端口需大于 1000（1000 以内没有空间让守护进程避开明文入口）');
			return true;
		};

		o = s.option(form.Value, 'webui_host', _('监听地址'),
			_('填 0.0.0.0 表示所有网卡（LAN）都可访问；也可填具体网卡地址。'));
		o.default = '0.0.0.0';
		o.rmempty = true;
		o.depends('webui', '1');
		o.placeholder = '0.0.0.0';

		o = s.option(form.Value, 'webui_pass', _('连接密码'),
			_('打开 WebUI 时需要输入，至少 8 个字符。留空则首次启用时自动生成随机密码，已有密码时保持不变。'));
		o.password = true;
		o.rmempty = true;
		o.depends('webui', '1');
		o.placeholder = _('至少 8 个字符');
		o.validate = function (section_id, value) {
			if (!value)
				return true;
			return (value.length >= 8) || _('密码至少需要 8 个字符');
		};

		o = s.option(form.Value, 'auto_start_tunnels', _('自动启动隧道'),
			_('填隧道 ID，多个用逗号分隔；守护进程启动后会自动启用这些隧道。') +
			'<br/>' + _('新建或修改隧道请到 ') +
			'<a href="https://www.natfrp.com/" target="_blank" rel="noopener noreferrer">natfrp.com 隧道列表</a>' +
			_('，或使用 ') +
			'<a href="https://www.natfrp.com/remote/v2" target="_blank" rel="noopener noreferrer">远程管理</a>' +
			_('。'));
		o.rmempty = true;
		o.placeholder = '29908344';

		o = s.option(form.Flag, 'check_update', _('自动检查更新'),
			_('每周检查一次启动器更新，下载更新会占用路由器存储空间。'));
		o.rmempty = true;
		o.depends('webui', '1');

		var card = this.renderCard();

		this.updateCard(status, secrets);

		var self = this;
		/* 口令单独取：拿到后缓存，轮询时继续复用，避免每轮都 fork 进程 */
		this.loadSecrets();

		poll.add(function () {
			return self.refresh();
		}, POLL_INTERVAL);

		/* form.Map.render() 返回 Promise，需要等表单节点就绪后再和状态卡拼装 */
		return Promise.resolve(m.render()).then(function (formNode) {
			return E('div', {}, [ E('style', { 'type': 'text/css' }, [ HERO_CSS ]), card, formNode ]);
		});
	},

	/* 顶部 hero：状态摘要 + WebUI 一键入口。窄屏自动改为单列、按钮等分整行。 */
	renderCard: function () {
		this.dotEl = E('span', { 'class': 'nf-dot' });
		this.stateEl = E('span', { 'class': 'nf-state' }, _('读取中…'));
		this.pidEl = E('span', { 'class': 'nf-meta' });
		this.verEl = E('span', { 'class': 'nf-chip' }, '—');
		this.bootEl = E('span', { 'class': 'nf-chip' }, '—');
		this.passEl = E('code', {}, '—');
		this.hintEl = E('span', { 'class': 'nf-hint' }, '');

		this.webuiBtn = E('a', {
			'class': 'btn cbi-button cbi-button-action nf-btn',
			'href': '#',
			'target': '_blank',
			'rel': 'noopener noreferrer'
		}, [ _('打开 WebUI') ]);

		this.copyBtn = E('button', {
			'class': 'btn cbi-button nf-btn',
			'type': 'button',
			'click': ui.createHandlerFn(this, function () {
				var self = this;
				var pass = this.passEl ? this.passEl.textContent : '';
				if (!pass || pass === '—')
					return;

				return copyText(pass).then(function () {
					flashButton(self.copyBtn, _('✓ 已复制'), 'nf-ok', 1600);
				}).catch(function () {
					flashButton(self.copyBtn, _('复制失败'), 'nf-bad', 2000);
				});
			})
		}, [ _('复制连接密码') ]);

		this.certBtn = E('button', {
			'class': 'btn cbi-button nf-cert',
			'type': 'button',
			'title': _('Windows：双击证书 → 安装到「受信任的根证书颁发机构」；Android/iOS：在系统设置里安装为 CA 证书'),
			'click': ui.createHandlerFn(this, function () {
				var self = this;

				return fs.exec(CTL, [ 'webui-cert' ]).then(function (res) {
					var data = parseJson(res ? String(res.stdout || '').trim() : '');
					if (!data || data.ok !== true || !data.pem)
						throw new Error((data && data.error) || _('读取证书失败'));

					var blob = new Blob([ data.pem ], { 'type': 'application/x-x509-ca-cert' });
					var url = URL.createObjectURL(blob);
					var a = E('a', { 'href': url, 'download': 'natfrp-webui.crt' });
					document.body.appendChild(a);
					a.click();
					document.body.removeChild(a);
					setTimeout(function () { URL.revokeObjectURL(url); }, 5000);

					flashButton(self.certBtn, _('已下载'), 'nf-ok', 1600);
				}).catch(function () {
					flashButton(self.certBtn, _('下载失败'), 'nf-bad', 2000);
				});
			})
		}, [ _('下载证书') ]);

		var head = E('div', { 'class': 'nf-hero-head' }, [
			this.dotEl,
			this.stateEl,
			this.pidEl,
			this.verEl,
			this.bootEl
		]);

		var body = E('div', { 'class': 'nf-hero-body' }, [
			head,
			E('div', { 'class': 'nf-hero-actions' }, [ this.webuiBtn, this.copyBtn ])
		]);

		var foot = E('div', { 'class': 'nf-hero-foot' }, [
			E('span', {}, [ _('连接密码'), '：', this.passEl ]),
			this.certBtn,
			this.hintEl
		]);

		/* 远程管理（natfrp 远程连接）用的口令，启用远程管理时才有意义 */
		this.remotePassEl = E('code', {}, '—');

		this.remoteCopyBtn = E('button', {
			'class': 'btn cbi-button nf-cert',
			'type': 'button',
			'click': ui.createHandlerFn(this, function () {
				var self = this;
				var pass = this.remotePassEl ? this.remotePassEl.textContent : '';
				if (!pass || pass === '—')
					return;
				return copyText(pass).then(function () {
					flashButton(self.remoteCopyBtn, _('✓ 已复制'), 'nf-ok', 1600);
				}).catch(function () {
					flashButton(self.remoteCopyBtn, _('复制失败'), 'nf-bad', 2000);
				});
			})
		}, [ _('复制') ]);

		this.remoteRow = E('div', { 'class': 'nf-hero-foot nf-remote', 'style': 'display:none' }, [
			E('span', {}, [ _('远程管理密码'), '：', this.remotePassEl ]),
			this.remoteCopyBtn
		]);

		this.cfgErrEl = E('div', { 'class': 'nf-hero-foot', 'style': 'display:none;color:#b3261e' }, '');

		this.heroEl = E('div', { 'class': 'nf-hero', 'id': 'natfrp-card' },
			[ this.cfgErrEl, body, foot, this.remoteRow ]);

		return this.heroEl;
	},

	updateCard: function (st, sec) {
		if (st)
			this.lastStatus = st;
		if (!this.heroEl)
			return;

		var running = !!(st && st.running);
		var webui = (st && st.webui) || {};
		var remote = (st && st.remote) || {};
		var secretsOk = (sec !== null && sec !== undefined);

		var port = parseInt(webui.port, 10) || 0;
		var password = (sec && sec.webui_pass) || '';
		var webuiReady = !!(webui.enabled && port > 0);
		var remoteReady = !!(remote.enabled && remote.keyed);
		var localUrl = webuiReady
			? ((webui.tls === false ? 'http' : 'https') + '://' + window.location.hostname + ':' + port + '/')
			: null;

		if (running)
			this.heroEl.classList.add('is-run');
		else
			this.heroEl.classList.remove('is-run');

		this.stateEl.textContent = running ? _('运行中') : _('已停止');
		this.pidEl.textContent = (st && st.pid) ? ('PID ' + st.pid) : '';
		this.verEl.textContent = (st && st.version) ? ('v' + st.version) : _('版本未知');
		this.bootEl.textContent = (st && st.enabled) ? _('开机自启') : _('未开机自启');
		if (st && st.config_error) {
			this.cfgErrEl.style.display = '';
			this.cfgErrEl.textContent = _('配置文件读取失败：%s').format(st.config_error);
		}
		else {
			this.cfgErrEl.style.display = 'none';
			this.cfgErrEl.textContent = '';
		}

		if (!secretsOk)
			this.passEl.textContent = _('需要写权限');
		else
			this.passEl.textContent = password || _('未设置');

		/* 主入口自动选目标：远程管理配好就走 natfrp 的远程页面，
		   否则打开本机 WebUI；都没配好时置灰。 */
		var target = remoteReady ? REMOTE_URL : localUrl;

		this.webuiBtn.textContent = remoteReady ? _('打开远程管理') : _('打开 WebUI');
		if (target) {
			this.webuiBtn.setAttribute('href', target);
			this.webuiBtn.setAttribute('title', target);
			this.webuiBtn.removeAttribute('aria-disabled');
			this.webuiBtn.style.pointerEvents = '';
			this.webuiBtn.style.opacity = '';
		}
		else {
			this.webuiBtn.setAttribute('href', '#');
			this.webuiBtn.removeAttribute('title');
			this.webuiBtn.setAttribute('aria-disabled', 'true');
			this.webuiBtn.style.pointerEvents = 'none';
			this.webuiBtn.style.opacity = '.5';
		}

		if (remoteReady) {
			var caps = [];
			if (remote.allow_config === false)
				caps.push(_('禁止改配置'));
			if (remote.allow_update === false)
				caps.push(_('禁止触发更新'));
			this.hintEl.textContent = (running
				? _('跳到 natfrp 远程管理页，用下方的远程管理密码连接。')
				: _('启动器未运行，远程管理需要服务在线。'))
				+ (caps.length ? ' ' + _('当前已限制：%s。').format(caps.join('、')) : '');
		}
		else if (webuiReady) {
			this.hintEl.textContent = running
				? ((webui.tls === false)
					? _('明文 HTTP，口令不加密，请在可信内网使用。')
					: _('自签名证书，下载后导入信任即不再提示。'))
				: _('服务已停止，WebUI 可能无法连接。');
		}
		else {
			this.hintEl.textContent = _('均未配置：启用 WebUI，或启用远程管理并设置 E2E 密码。');
		}

		if (password)
			this.copyBtn.removeAttribute('disabled');
		else
			this.copyBtn.setAttribute('disabled', 'disabled');

		this.certBtn.style.display = webuiReady ? '' : 'none';

		this.remoteRow.style.display = remote.enabled ? '' : 'none';
		this.remotePassEl.textContent = secretsOk
			? ((sec && sec.remote_pass) || _('未保存'))
			: _('需要写权限');
		if (secretsOk && sec && sec.remote_pass)
			this.remoteCopyBtn.removeAttribute('disabled');
		else
			this.remoteCopyBtn.setAttribute('disabled', 'disabled');
	},
	/* 只在口令尚未取到或明确需要刷新时调用 */
	loadSecrets: function () {
		var self = this;
		return readSecrets().then(function (sec) {
			self.secrets = sec;
			if (self.lastStatus)
				self.updateCard(self.lastStatus, sec);
			return sec;
		});
	},

	refresh: function () {
		var self = this;
		return readStatus().then(function (st) {
			self.lastStatus = st;
			self.updateCard(st, self.secrets);
			return st;
		});
	},

	/*
	 * 自己控制「提交 uci -> 同步守护进程配置 -> 刷新页面」的顺序。
	 * 直接复用 ui.changes.apply() 会在提交后自行 reload，可能把我们后面的
	 * natfrpctl apply 打断，导致开关状态和守护进程配置对不上。
	 */
	handleSaveApply: function (ev) {
		return this.handleSave(ev).then(function () {
			return uci.apply();
		}).then(function () {
			return applySettings();
		}).then(function () {
			window.location.reload();
		}).catch(function (err) {
			ui.addNotification(null,
				E('p', _('应用配置失败：%s').format(err && err.message ? err.message : err)), 'error');
		});
	}
});