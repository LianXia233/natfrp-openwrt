/*
 * 明文 HTTP 下的 WebCrypto 兼容层。
 *
 * 启动器 WebUI 登录时用 crypto.subtle 做 HMAC-SHA256，而浏览器只在安全上下文
 * （HTTPS / localhost）提供该接口。经明文入口访问时先用这里补齐，页面其余逻辑
 * 不受影响。
 */
(function (global) {
	'use strict';

	var K = [
		0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
		0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
		0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
		0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
		0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
		0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
		0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
		0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
	];

	function rotr(x, n) {
		return (x >>> n) | (x << (32 - n));
	}

	function sha256(bytes) {
		var H = [ 0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
			0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19 ];
		var len = bytes.length;
		var withPad = ((len + 9 + 63) >> 6) << 6;
		var buf = new Uint8Array(withPad);
		buf.set(bytes);
		buf[len] = 0x80;

		var bitsHi = Math.floor(len / 0x20000000);
		var bitsLo = (len << 3) >>> 0;
		buf[withPad - 8] = (bitsHi >>> 24) & 0xff;
		buf[withPad - 7] = (bitsHi >>> 16) & 0xff;
		buf[withPad - 6] = (bitsHi >>> 8) & 0xff;
		buf[withPad - 5] = bitsHi & 0xff;
		buf[withPad - 4] = (bitsLo >>> 24) & 0xff;
		buf[withPad - 3] = (bitsLo >>> 16) & 0xff;
		buf[withPad - 2] = (bitsLo >>> 8) & 0xff;
		buf[withPad - 1] = bitsLo & 0xff;

		var W = new Int32Array(64);
		for (var off = 0; off < withPad; off += 64) {
			var i;
			for (i = 0; i < 16; i++) {
				W[i] = (buf[off + i * 4] << 24) | (buf[off + i * 4 + 1] << 16) |
					(buf[off + i * 4 + 2] << 8) | buf[off + i * 4 + 3];
			}
			for (i = 16; i < 64; i++) {
				var s0 = rotr(W[i - 15], 7) ^ rotr(W[i - 15], 18) ^ (W[i - 15] >>> 3);
				var s1 = rotr(W[i - 2], 17) ^ rotr(W[i - 2], 19) ^ (W[i - 2] >>> 10);
				W[i] = (W[i - 16] + s0 + W[i - 7] + s1) | 0;
			}

			var a = H[0], b = H[1], c = H[2], d = H[3];
			var e = H[4], f = H[5], g = H[6], h = H[7];

			for (i = 0; i < 64; i++) {
				var S1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
				var ch = (e & f) ^ (~e & g);
				var t1 = (h + S1 + ch + K[i] + W[i]) | 0;
				var S0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
				var maj = (a & b) ^ (a & c) ^ (b & c);
				var t2 = (S0 + maj) | 0;

				h = g; g = f; f = e;
				e = (d + t1) | 0;
				d = c; c = b; b = a;
				a = (t1 + t2) | 0;
			}

			H[0] = (H[0] + a) | 0; H[1] = (H[1] + b) | 0;
			H[2] = (H[2] + c) | 0; H[3] = (H[3] + d) | 0;
			H[4] = (H[4] + e) | 0; H[5] = (H[5] + f) | 0;
			H[6] = (H[6] + g) | 0; H[7] = (H[7] + h) | 0;
		}

		var out = new Uint8Array(32);
		for (var j = 0; j < 8; j++) {
			out[j * 4] = (H[j] >>> 24) & 0xff;
			out[j * 4 + 1] = (H[j] >>> 16) & 0xff;
			out[j * 4 + 2] = (H[j] >>> 8) & 0xff;
			out[j * 4 + 3] = H[j] & 0xff;
		}
		return out;
	}

	function hmacSha256(key, msg) {
		if (key.length > 64)
			key = sha256(key);

		var inner = new Uint8Array(64 + msg.length);
		var outer = new Uint8Array(64 + 32);
		var i;

		for (i = 0; i < 64; i++) {
			var k = i < key.length ? key[i] : 0;
			inner[i] = k ^ 0x36;
			outer[i] = k ^ 0x5c;
		}

		inner.set(msg, 64);
		outer.set(sha256(inner), 64);

		return sha256(outer);
	}

	function toBytes(data) {
		if (data instanceof Uint8Array)
			return data;
		if (data instanceof ArrayBuffer)
			return new Uint8Array(data);
		if (data && data.buffer)
			return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
		return new Uint8Array(0);
	}

	function makeShim() {
		return {
			importKey: function (format, keyData, algo) {
				if (format !== 'raw' || !algo || String(algo.name).toUpperCase() !== 'HMAC')
					return Promise.reject(new Error('unsupported importKey'));
				return Promise.resolve({ __nfHmacKey: toBytes(keyData).slice() });
			},
			sign: function (algo, key, data) {
				if (!key || !key.__nfHmacKey)
					return Promise.reject(new Error('unsupported key'));
				var mac = hmacSha256(key.__nfHmacKey, toBytes(data));
				return Promise.resolve(mac.buffer);
			},
			verify: function () {
				return Promise.reject(new Error('verify not supported'));
			},
			digest: function (algo, data) {
				return Promise.resolve(sha256(toBytes(data)).buffer);
			}
		};
	}

	/* 只在缺少 crypto.subtle 时接管；HTTPS 下保持浏览器原生实现 */
	function install(target) {
		if (!target)
			return null;
		if (target.subtle)
			return target.subtle;

		var shim = makeShim();
		try {
			Object.defineProperty(target, 'subtle', {
				get: function () { return shim; },
				configurable: true
			});
		}
		catch (e) {
			try { target.subtle = shim; } catch (e2) { return null; }
		}
		return shim;
	}

	if (typeof module === 'object' && module.exports)
		module.exports = { sha256: sha256, hmacSha256: hmacSha256, install: install };

	if (global && global.crypto)
		install(global.crypto);
})(typeof window !== 'undefined' ? window : null);
