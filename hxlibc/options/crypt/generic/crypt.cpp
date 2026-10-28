#include <crypt.h>
#include <errno.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

namespace {

struct Sha512 {
	uint64_t state[8];
	uint64_t length;
	unsigned char block[128];
	size_t used;
};

constexpr uint64_t round_constants[80] = {
	0x428a2f98d728ae22ULL, 0x7137449123ef65cdULL, 0xb5c0fbcfec4d3b2fULL, 0xe9b5dba58189dbbcULL,
	0x3956c25bf348b538ULL, 0x59f111f1b605d019ULL, 0x923f82a4af194f9bULL, 0xab1c5ed5da6d8118ULL,
	0xd807aa98a3030242ULL, 0x12835b0145706fbeULL, 0x243185be4ee4b28cULL, 0x550c7dc3d5ffb4e2ULL,
	0x72be5d74f27b896fULL, 0x80deb1fe3b1696b1ULL, 0x9bdc06a725c71235ULL, 0xc19bf174cf692694ULL,
	0xe49b69c19ef14ad2ULL, 0xefbe4786384f25e3ULL, 0x0fc19dc68b8cd5b5ULL, 0x240ca1cc77ac9c65ULL,
	0x2de92c6f592b0275ULL, 0x4a7484aa6ea6e483ULL, 0x5cb0a9dcbd41fbd4ULL, 0x76f988da831153b5ULL,
	0x983e5152ee66dfabULL, 0xa831c66d2db43210ULL, 0xb00327c898fb213fULL, 0xbf597fc7beef0ee4ULL,
	0xc6e00bf33da88fc2ULL, 0xd5a79147930aa725ULL, 0x06ca6351e003826fULL, 0x142929670a0e6e70ULL,
	0x27b70a8546d22ffcULL, 0x2e1b21385c26c926ULL, 0x4d2c6dfc5ac42aedULL, 0x53380d139d95b3dfULL,
	0x650a73548baf63deULL, 0x766a0abb3c77b2a8ULL, 0x81c2c92e47edaee6ULL, 0x92722c851482353bULL,
	0xa2bfe8a14cf10364ULL, 0xa81a664bbc423001ULL, 0xc24b8b70d0f89791ULL, 0xc76c51a30654be30ULL,
	0xd192e819d6ef5218ULL, 0xd69906245565a910ULL, 0xf40e35855771202aULL, 0x106aa07032bbd1b8ULL,
	0x19a4c116b8d2d0c8ULL, 0x1e376c085141ab53ULL, 0x2748774cdf8eeb99ULL, 0x34b0bcb5e19b48a8ULL,
	0x391c0cb3c5c95a63ULL, 0x4ed8aa4ae3418acbULL, 0x5b9cca4f7763e373ULL, 0x682e6ff3d6b2b8a3ULL,
	0x748f82ee5defb2fcULL, 0x78a5636f43172f60ULL, 0x84c87814a1f0ab72ULL, 0x8cc702081a6439ecULL,
	0x90befffa23631e28ULL, 0xa4506cebde82bde9ULL, 0xbef9a3f7b2c67915ULL, 0xc67178f2e372532bULL,
	0xca273eceea26619cULL, 0xd186b8c721c0c207ULL, 0xeada7dd6cde0eb1eULL, 0xf57d4f7fee6ed178ULL,
	0x06f067aa72176fbaULL, 0x0a637dc5a2c898a6ULL, 0x113f9804bef90daeULL, 0x1b710b35131c471bULL,
	0x28db77f523047d84ULL, 0x32caab7b40c72493ULL, 0x3c9ebe0a15c9bebcULL, 0x431d67c49c100d4cULL,
	0x4cc5d4becb3e42b6ULL, 0x597f299cfc657e2aULL, 0x5fcb6fab3ad6faecULL, 0x6c44198c4a475817ULL,
};

uint64_t rotate_right(uint64_t x, int n) {
	return (x >> n) | (x << (64 - n));
}

void sha512_compress(Sha512 &ctx, const unsigned char *data) {
	uint64_t w[80];
	for (int i = 0; i < 16; i++) {
		uint64_t v = 0;
		for (int k = 0; k < 8; k++)
			v = (v << 8) | data[i * 8 + k];
		w[i] = v;
	}
	for (int i = 16; i < 80; i++) {
		uint64_t s0 = rotate_right(w[i - 15], 1) ^ rotate_right(w[i - 15], 8) ^ (w[i - 15] >> 7);
		uint64_t s1 = rotate_right(w[i - 2], 19) ^ rotate_right(w[i - 2], 61) ^ (w[i - 2] >> 6);
		w[i] = w[i - 16] + s0 + w[i - 7] + s1;
	}
	uint64_t a = ctx.state[0], b = ctx.state[1], c = ctx.state[2], d = ctx.state[3];
	uint64_t e = ctx.state[4], f = ctx.state[5], g = ctx.state[6], h = ctx.state[7];
	for (int i = 0; i < 80; i++) {
		uint64_t s1 = rotate_right(e, 14) ^ rotate_right(e, 18) ^ rotate_right(e, 41);
		uint64_t choice = (e & f) ^ (~e & g);
		uint64_t t1 = h + s1 + choice + round_constants[i] + w[i];
		uint64_t s0 = rotate_right(a, 28) ^ rotate_right(a, 34) ^ rotate_right(a, 39);
		uint64_t majority = (a & b) ^ (a & c) ^ (b & c);
		uint64_t t2 = s0 + majority;
		h = g;
		g = f;
		f = e;
		e = d + t1;
		d = c;
		c = b;
		b = a;
		a = t1 + t2;
	}
	ctx.state[0] += a;
	ctx.state[1] += b;
	ctx.state[2] += c;
	ctx.state[3] += d;
	ctx.state[4] += e;
	ctx.state[5] += f;
	ctx.state[6] += g;
	ctx.state[7] += h;
}

void sha512_init(Sha512 &ctx) {
	static constexpr uint64_t initial[8] = {
		0x6a09e667f3bcc908ULL, 0xbb67ae8584caa73bULL, 0x3c6ef372fe94f82bULL, 0xa54ff53a5f1d36f1ULL,
		0x510e527fade682d1ULL, 0x9b05688c2b3e6c1fULL, 0x1f83d9abfb41bd6bULL, 0x5be0cd19137e2179ULL,
	};
	memcpy(ctx.state, initial, sizeof initial);
	ctx.length = 0;
	ctx.used = 0;
}

void sha512_update(Sha512 &ctx, const void *data, size_t len) {
	const unsigned char *bytes = static_cast<const unsigned char *>(data);
	ctx.length += len;
	while (len) {
		size_t take = 128 - ctx.used;
		if (take > len)
			take = len;
		memcpy(ctx.block + ctx.used, bytes, take);
		ctx.used += take;
		bytes += take;
		len -= take;
		if (ctx.used == 128) {
			sha512_compress(ctx, ctx.block);
			ctx.used = 0;
		}
	}
}

void sha512_final(Sha512 &ctx, unsigned char out[64]) {
	uint64_t bits = ctx.length * 8;
	unsigned char pad = 0x80;
	sha512_update(ctx, &pad, 1);
	unsigned char zero = 0;
	while (ctx.used != 112)
		sha512_update(ctx, &zero, 1);
	unsigned char tail[16] = {};
	for (int i = 0; i < 8; i++)
		tail[15 - i] = static_cast<unsigned char>(bits >> (i * 8));
	sha512_update(ctx, tail, 16);
	for (int i = 0; i < 8; i++)
		for (int k = 0; k < 8; k++)
			out[i * 8 + k] = static_cast<unsigned char>(ctx.state[i] >> (56 - k * 8));
}

void add_repeated(Sha512 &ctx, const unsigned char *digest, size_t len) {
	for (; len > 64; len -= 64)
		sha512_update(ctx, digest, 64);
	sha512_update(ctx, digest, len);
}

void wipe(void *data, size_t len) {
	volatile unsigned char *bytes = static_cast<volatile unsigned char *>(data);
	while (len--)
		*bytes++ = 0;
}

constexpr char alphabet[] = "./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

char *encode(char *out, unsigned b2, unsigned b1, unsigned b0, int count) {
	unsigned w = (b2 << 16) | (b1 << 8) | b0;
	while (count-- > 0) {
		*out++ = alphabet[w & 0x3f];
		w >>= 6;
	}
	return out;
}

constexpr size_t output_size = 3 + 7 + 10 + 1 + 16 + 1 + 86 + 1;

char *sha512_crypt(const char *key, const char *setting, char *output) {
	const char *salt = setting + 3;
	unsigned long rounds = 5000;
	bool custom_rounds = false;
	if (strncmp(salt, "rounds=", 7) == 0) {
		char *end;
		unsigned long wanted = strtoul(salt + 7, &end, 10);
		if (*end != '$')
			return nullptr;
		rounds = wanted < 1000 ? 1000 : wanted > 999999999 ? 999999999 : wanted;
		custom_rounds = true;
		salt = end + 1;
	}
	size_t salt_len = strcspn(salt, "$");
	if (salt_len > 16)
		salt_len = 16;
	size_t key_len = strlen(key);

	Sha512 ctx;
	unsigned char alternate[64];
	sha512_init(ctx);
	sha512_update(ctx, key, key_len);
	sha512_update(ctx, salt, salt_len);
	sha512_update(ctx, key, key_len);
	sha512_final(ctx, alternate);

	unsigned char digest[64];
	sha512_init(ctx);
	sha512_update(ctx, key, key_len);
	sha512_update(ctx, salt, salt_len);
	add_repeated(ctx, alternate, key_len);
	for (size_t n = key_len; n > 0; n >>= 1) {
		if (n & 1)
			sha512_update(ctx, alternate, 64);
		else
			sha512_update(ctx, key, key_len);
	}
	sha512_final(ctx, digest);

	unsigned char key_digest[64];
	sha512_init(ctx);
	for (size_t i = 0; i < key_len; i++)
		sha512_update(ctx, key, key_len);
	sha512_final(ctx, key_digest);
	unsigned char *key_bytes = static_cast<unsigned char *>(malloc(key_len ? key_len : 1));
	if (!key_bytes)
		return nullptr;
	for (size_t i = 0; i < key_len; i += 64)
		memcpy(key_bytes + i, key_digest, key_len - i < 64 ? key_len - i : 64);

	unsigned char salt_digest[64];
	sha512_init(ctx);
	for (size_t i = 0; i < 16u + digest[0]; i++)
		sha512_update(ctx, salt, salt_len);
	sha512_final(ctx, salt_digest);
	unsigned char salt_bytes[16];
	memcpy(salt_bytes, salt_digest, salt_len);

	for (unsigned long round = 0; round < rounds; round++) {
		sha512_init(ctx);
		if (round & 1)
			sha512_update(ctx, key_bytes, key_len);
		else
			sha512_update(ctx, digest, 64);
		if (round % 3)
			sha512_update(ctx, salt_bytes, salt_len);
		if (round % 7)
			sha512_update(ctx, key_bytes, key_len);
		if (round & 1)
			sha512_update(ctx, digest, 64);
		else
			sha512_update(ctx, key_bytes, key_len);
		sha512_final(ctx, digest);
	}
	wipe(key_bytes, key_len);
	free(key_bytes);

	char *p = output;
	memcpy(p, "$6$", 3);
	p += 3;
	if (custom_rounds) {
		char number[16];
		int digits = 0;
		for (unsigned long r = rounds; r; r /= 10)
			number[digits++] = static_cast<char>('0' + r % 10);
		memcpy(p, "rounds=", 7);
		p += 7;
		while (digits)
			*p++ = number[--digits];
		*p++ = '$';
	}
	memcpy(p, salt, salt_len);
	p += salt_len;
	*p++ = '$';
	static constexpr unsigned char order[21][3] = {
		{0, 21, 42}, {22, 43, 1}, {44, 2, 23}, {3, 24, 45}, {25, 46, 4}, {47, 5, 26}, {6, 27, 48},
		{28, 49, 7}, {50, 8, 29}, {9, 30, 51}, {31, 52, 10}, {53, 11, 32}, {12, 33, 54}, {34, 55, 13},
		{56, 14, 35}, {15, 36, 57}, {37, 58, 16}, {59, 17, 38}, {18, 39, 60}, {40, 61, 19}, {62, 20, 41},
	};
	for (const auto &group : order)
		p = encode(p, digest[group[0]], digest[group[1]], digest[group[2]], 4);
	p = encode(p, 0, 0, digest[63], 2);
	*p = 0;
	wipe(digest, sizeof digest);
	wipe(alternate, sizeof alternate);
	wipe(key_digest, sizeof key_digest);
	return output;
}

} // namespace

char *crypt(const char *key, const char *setting) {
	static char output[output_size];
	if (!key || !setting || strncmp(setting, "$6$", 3) != 0) {
		errno = EINVAL;
		return nullptr;
	}
	char *result = sha512_crypt(key, setting, output);
	if (!result)
		errno = EINVAL;
	return result;
}
