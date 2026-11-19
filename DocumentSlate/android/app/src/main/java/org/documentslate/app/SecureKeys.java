package org.documentslate.app;

import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.security.keystore.KeyProtection;
import android.security.keystore.UserNotAuthenticatedException;

import java.io.ByteArrayInputStream;
import java.security.KeyFactory;
import java.security.KeyPair;
import java.security.KeyPairGenerator;
import java.security.KeyStore;
import java.security.PrivateKey;
import java.security.Signature;
import java.security.cert.Certificate;
import java.security.cert.CertificateFactory;
import java.security.spec.PKCS8EncodedKeySpec;
import java.security.spec.RSAKeyGenParameterSpec;
import java.math.BigInteger;

/**
 * Digital-ID private keys in the Android Keystore (the phone's hardware-backed key vault).
 *
 * Keys are stored "user authentication required": they only sign within {@link #AUTH_SECONDS}
 * seconds of the user unlocking with fingerprint / face / PIN / pattern / password (Rust shows
 * that prompt first through MainActivity.authenticate()). Keys can't be read back out, not even
 * by this app. If the user removes their screen lock, Android deletes these keys.
 *
 * Certificates are kept by Rust (files/digital-ids.json); the Keystore only holds the keys.
 * Every method returns null / false on failure and leaves the reason in {@link #lastError}
 * ("auth" when the user must unlock first).
 */
final class SecureKeys {
    private SecureKeys() {}

    static final int AUTH_SECONDS = 30;
    private static final String STORE = "AndroidKeyStore";
    static volatile String lastError;

    private static KeyStore store() throws Exception {
        KeyStore ks = KeyStore.getInstance(STORE);
        ks.load(null);
        return ks;
    }

    @SuppressWarnings("deprecation")
    private static void requireAuth(KeyProtection.Builder b) {
        b.setUserAuthenticationRequired(true);
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            b.setUserAuthenticationParameters(AUTH_SECONDS,
                    KeyProperties.AUTH_BIOMETRIC_STRONG | KeyProperties.AUTH_DEVICE_CREDENTIAL);
        } else {
            b.setUserAuthenticationValidityDurationSeconds(AUTH_SECONDS);
        }
    }

    @SuppressWarnings("deprecation")
    private static void requireAuth(KeyGenParameterSpec.Builder b) {
        b.setUserAuthenticationRequired(true);
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            b.setUserAuthenticationParameters(AUTH_SECONDS,
                    KeyProperties.AUTH_BIOMETRIC_STRONG | KeyProperties.AUTH_DEVICE_CREDENTIAL);
        } else {
            b.setUserAuthenticationValidityDurationSeconds(AUTH_SECONDS);
        }
    }

    /** Move a private key (PKCS#8, "RSA" or "EC") with its certificate into the Keystore. */
    static boolean importKey(String alias, byte[] pkcs8, String family, byte[] certDer) {
        try {
            PrivateKey key = KeyFactory.getInstance(family).generatePrivate(new PKCS8EncodedKeySpec(pkcs8));
            Certificate cert = CertificateFactory.getInstance("X.509").generateCertificate(new ByteArrayInputStream(certDer));
            KeyProtection.Builder b = new KeyProtection.Builder(KeyProperties.PURPOSE_SIGN)
                    .setDigests(KeyProperties.DIGEST_SHA256, KeyProperties.DIGEST_SHA384, KeyProperties.DIGEST_SHA512);
            if ("RSA".equals(family)) {
                b.setSignaturePaddings(KeyProperties.SIGNATURE_PADDING_RSA_PKCS1);
            }
            requireAuth(b);
            store().setEntry(alias, new KeyStore.PrivateKeyEntry(key, new Certificate[]{cert}), b.build());
            return true;
        } catch (Exception e) {
            lastError = describe(e);
            return false;
        }
    }

    /** A new 2048-bit RSA key made inside the Keystore; returns its public key (SPKI DER). */
    static byte[] generateRsa(String alias) {
        try {
            KeyGenParameterSpec.Builder b = new KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_SIGN)
                    .setAlgorithmParameterSpec(new RSAKeyGenParameterSpec(2048, RSAKeyGenParameterSpec.F4))
                    .setDigests(KeyProperties.DIGEST_SHA256, KeyProperties.DIGEST_SHA384, KeyProperties.DIGEST_SHA512)
                    .setSignaturePaddings(KeyProperties.SIGNATURE_PADDING_RSA_PKCS1)
                    .setCertificateSerialNumber(BigInteger.ONE)
                    .setCertificateSubject(new javax.security.auth.x500.X500Principal("CN=Document Slate"));
            requireAuth(b);
            KeyPairGenerator g = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_RSA, STORE);
            g.initialize(b.build());
            KeyPair kp = g.generateKeyPair();
            return kp.getPublic().getEncoded();
        } catch (Exception e) {
            lastError = describe(e);
            return null;
        }
    }

    /**
     * Sign {@code msg} with the key {@code alias}: digest "SHA-256" / "SHA-384" / "SHA-512",
     * PKCS#1 v1.5 for RSA keys, DER-encoded ECDSA for EC keys.
     */
    static byte[] sign(String alias, String digest, byte[] msg) {
        try {
            KeyStore ks = store();
            KeyStore.Entry entry = ks.getEntry(alias, null);
            if (!(entry instanceof KeyStore.PrivateKeyEntry)) {
                lastError = "the key for this digital ID is gone (was the screen lock removed?)";
                return null;
            }
            PrivateKey key = ((KeyStore.PrivateKeyEntry) entry).getPrivateKey();
            String d = digest.replace("-", "");
            String alg = d + "with" + ("EC".equals(key.getAlgorithm()) ? "ECDSA" : "RSA");
            Signature s = Signature.getInstance(alg);
            s.initSign(key);
            s.update(msg);
            return s.sign();
        } catch (UserNotAuthenticatedException e) {
            lastError = "auth";
            return null;
        } catch (Exception e) {
            lastError = describe(e);
            return null;
        }
    }

    static boolean has(String alias) {
        try {
            return store().containsAlias(alias);
        } catch (Exception e) {
            lastError = describe(e);
            return false;
        }
    }

    static boolean delete(String alias) {
        try {
            store().deleteEntry(alias);
            return true;
        } catch (Exception e) {
            lastError = describe(e);
            return false;
        }
    }

    private static String describe(Exception e) {
        if (e instanceof UserNotAuthenticatedException) {
            return "auth";
        }
        Throwable c = e;
        while (c.getCause() != null && c.getCause() != c) {
            if (c.getCause() instanceof UserNotAuthenticatedException) {
                return "auth";
            }
            c = c.getCause();
        }
        String m = e.getMessage();
        if (m != null && m.contains("secure lock screen")) {
            return "nolock";
        }
        return e.getClass().getSimpleName() + (m == null ? "" : ": " + m);
    }
}
