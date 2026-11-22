// 校验 rust/inject.js 里的 WebCrypto 兼容层：
// 用它算出的 HMAC-SHA256 必须与 Node 的 crypto 完全一致。

const crypto = require('node:crypto');
const path = require('node:path');

const mod = require(path.join(__dirname, '..', 'rust', 'inject.js'));
const { hmacSha256, sha256, install } = mod;

let cases = 0;
let bad = 0;

function check(name, want, got) {
	cases++;
	if (want !== got) {
		bad++;
		console.error(`mismatch: ${name}\n  want ${want}\n  got  ${got}`);
	}
}

for (let i = 0; i < 40; i++) {
	const key = crypto.randomBytes(i % 33);
	const msg = crypto.randomBytes((i * 7) % 129);
	const want = crypto.createHmac('sha256', key).update(msg).digest('hex');
	const got = Buffer.from(hmacSha256(new Uint8Array(key), new Uint8Array(msg))).toString('hex');
	check(`hmac #${i}`, want, got);
}

for (let i = 0; i < 20; i++) {
	const data = crypto.randomBytes((i * 13) % 257);
	const want = crypto.createHash('sha256').update(data).digest('hex');
	const got = Buffer.from(sha256(new Uint8Array(data))).toString('hex');
	check(`sha256 #${i}`, want, got);
}

(async () => {
	const fake = {};
	install(fake);

	const enc = new TextEncoder();
	const key = await fake.subtle.importKey('raw', enc.encode('pw'),
		{ name: 'HMAC', hash: { name: 'SHA-256' } }, false, [ 'sign' ]);
	const sig = await fake.subtle.sign('HMAC', key, enc.encode('challenge'));
	const got = Buffer.from(sig).toString('hex');
	const want = crypto.createHmac('sha256', 'pw').update('challenge').digest('hex');
	check('subtle shim', want, got);

	if (bad > 0) {
		console.error(`FAILED ${bad}/${cases}`);
		process.exit(1);
	}
	console.log(`inject.js OK (${cases} cases)`);
})();
