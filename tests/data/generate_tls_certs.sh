#!/usr/bin/env bash
openssl req -subj '/CN=localhost' -x509 -newkey rsa:4096 -keyout key_pkcs8.pem -out cert_rsa.pem -nodes -days 3650
openssl rsa -in key_pkcs8.pem -out key_pkcs1.pem
openssl req -subj '/CN=localhost' -x509 -nodes -newkey ec -pkeyopt ec_paramgen_curve:secp384r1 -keyout key_ec.pem -out cert_ec.pem -days 3650

# Stores a client certificate issued by the given CA together with its key, as reqwest expects it
client_cert() { # file CA CA-key
    openssl req -subj "/CN=${1%.pem}" -nodes -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -keyout key.tmp -out csr.tmp
    openssl x509 -req -in csr.tmp -CA "$2" -CAkey "$3" -days 3650 -extfile <(echo extendedKeyUsage=clientAuth) -out cert.tmp
    cat key.tmp cert.tmp > "$1"
    rm key.tmp csr.tmp cert.tmp
}
openssl req -subj '/CN=Client CA' -x509 -nodes -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -keyout client_ca_key.pem -out client_ca.pem -days 3650
client_cert client.pem client_ca.pem client_ca_key.pem
# Issued by a CA that miniserve isn't told to trust
openssl req -subj '/CN=Other CA' -x509 -nodes -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -keyout other_ca_key.tmp -out other_ca.tmp -days 3650
client_cert client_untrusted.pem other_ca.tmp other_ca_key.tmp
rm other_ca.tmp other_ca_key.tmp

# Revoked in the client CA's CRL
client_cert client_revoked.pem client_ca.pem client_ca_key.pem
mkdir ca.tmp && touch ca.tmp/index.txt && echo 01 > ca.tmp/crlnumber
printf '[ca]\ndefault_ca = client_ca\n[client_ca]\ndatabase = ca.tmp/index.txt\ncrlnumber = ca.tmp/crlnumber\ndefault_md = sha256\n' > ca.tmp/ca.cnf
openssl ca -config ca.tmp/ca.cnf -cert client_ca.pem -keyfile client_ca_key.pem -revoke client_revoked.pem
openssl ca -config ca.tmp/ca.cnf -cert client_ca.pem -keyfile client_ca_key.pem -gencrl -crldays 3650 -out client_crl.pem
rm -r ca.tmp
