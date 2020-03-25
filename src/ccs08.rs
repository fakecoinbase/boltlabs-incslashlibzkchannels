/*
Implementation of the ZK Range Proof scheme, based on:
Efficient Protocols for Set Membership and Range Proofs
Jan Camenisch, Rafik Chaabouni, and abhi shelat
Asiacrypt 2008
*/

use super::*;
use cl::{
    setup, BlindKeyPair, BlindPublicKey, ProofState, PublicParams, Signature, SignatureProof,
};
use ff::PrimeField;
use pairing::{CurveProjective, Engine};
use ped92::{CSMultiParams, Commitment};
use rand::Rng;
use std::collections::HashMap;

/**
paramsUL contains elements generated by the verifier, which are necessary for the prover.
This must be computed in a trusted setup.
*/
#[derive(Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "<E as ff::ScalarEngine>::Fr: serde::Serialize, <E as pairing::Engine>::G1: serde::Serialize, <E as pairing::Engine>::G2: serde::Serialize"
))]
#[serde(bound(
    deserialize = "<E as ff::ScalarEngine>::Fr: serde::Deserialize<'de>, <E as pairing::Engine>::G1: serde::Deserialize<'de>, <E as pairing::Engine>::G2: serde::Deserialize<'de>"
))]
pub struct ParamsUL<E: Engine> {
    pub mpk: PublicParams<E>,
    pub signatures: HashMap<String, Signature<E>>,
    pub csParams: CSMultiParams<E>,
    pk: BlindPublicKey<E>,
    // u determines the amount of signatures we need in the public params.
    // Each signature can be compressed to just 1 field element of 256 bits.
    // Then the parameters have minimum size equal to 256*u bits.
    u: i64,
    // l determines how many pairings we need to compute, then in order to improve
    // verifier`s performance we want to minize it.
    // Namely, we have 2*l pairings for the prover and 3*l for the verifier.
    l: i64,
}

/**
paramsUL contains elements generated by the verifier, which are necessary for the prover.
This must be computed in a trusted setup.
*/
#[derive(Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "<E as ff::ScalarEngine>::Fr: serde::Serialize, <E as pairing::Engine>::G1: serde::Serialize, <E as pairing::Engine>::G2: serde::Serialize"
))]
#[serde(bound(
    deserialize = "<E as ff::ScalarEngine>::Fr: serde::Deserialize<'de>, <E as pairing::Engine>::G1: serde::Deserialize<'de>, <E as pairing::Engine>::G2: serde::Deserialize<'de>"
))]
pub struct SecretParamsUL<E: Engine> {
    pub pubParams: ParamsUL<E>,
    pub kp: BlindKeyPair<E>,
}

#[derive(Clone)]
pub struct ProofULState<E: Engine> {
    pub decx: Vec<i64>,
    pub proofStates: Vec<ProofState<E>>,
    pub V: Vec<Signature<E>>,
    pub D: E::G1,
    pub m: E::Fr,
    pub s: Vec<E::Fr>,
}

/**
proofUL contains the necessary elements for the ZK range proof with range [0,u^l).
*/
#[derive(Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "<E as ff::ScalarEngine>::Fr: serde::Serialize, \
<E as pairing::Engine>::G1: serde::Serialize, \
<E as pairing::Engine>::G2: serde::Serialize, \
<E as pairing::Engine>::Fqk: serde::Serialize"))]
#[serde(
    bound(deserialize = "<E as ff::ScalarEngine>::Fr: serde::Deserialize<'de>, \
<E as pairing::Engine>::G1: serde::Deserialize<'de>, \
<E as pairing::Engine>::G2: serde::Deserialize<'de>, \
<E as pairing::Engine>::Fqk: serde::Deserialize<'de>")
)]
pub struct ProofUL<E: Engine> {
    pub V: Vec<Signature<E>>,
    pub D: E::G1,
    pub comm: Commitment<E>,
    pub sigProofs: Vec<SignatureProof<E>>,
    pub zr: E::Fr,
    pub zs: Vec<E::Fr>,
}

#[derive(Clone)]
pub struct RangeProofState<E: Engine> {
    pub com1: Commitment<E>,
    pub ps1: ProofULState<E>,
    pub com2: Commitment<E>,
    pub ps2: ProofULState<E>,
}

/**
RangeProof contains the necessary elements for the ZK range proof.
*/
#[derive(Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "<E as ff::ScalarEngine>::Fr: serde::Serialize, \
<E as pairing::Engine>::G1: serde::Serialize, \
<E as pairing::Engine>::G2: serde::Serialize, \
<E as pairing::Engine>::Fqk: serde::Serialize"))]
#[serde(
    bound(deserialize = "<E as ff::ScalarEngine>::Fr: serde::Deserialize<'de>, \
<E as pairing::Engine>::G1: serde::Deserialize<'de>, \
<E as pairing::Engine>::G2: serde::Deserialize<'de>, \
<E as pairing::Engine>::Fqk: serde::Deserialize<'de>")
)]
pub struct RangeProof<E: Engine> {
    pub p1: ProofUL<E>,
    pub p2: ProofUL<E>,
}

/**
params contains elements generated by the verifier, which are necessary for the prover.
This must be computed in a trusted setup.
*/
#[derive(Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "<E as ff::ScalarEngine>::Fr: serde::Serialize, <E as pairing::Engine>::G1: serde::Serialize, <E as pairing::Engine>::G2: serde::Serialize"
))]
#[serde(bound(
    deserialize = "<E as ff::ScalarEngine>::Fr: serde::Deserialize<'de>, <E as pairing::Engine>::G1: serde::Deserialize<'de>, <E as pairing::Engine>::G2: serde::Deserialize<'de>"
))]
pub struct RPPublicParams<E: Engine> {
    pub p: ParamsUL<E>,
    pub a: i64,
    pub b: i64,
}

/**
params contains elements generated by the verifier, which are necessary for the prover.
This must be computed in a trusted setup.
*/
#[derive(Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "<E as ff::ScalarEngine>::Fr: serde::Serialize, <E as pairing::Engine>::G1: serde::Serialize, <E as pairing::Engine>::G2: serde::Serialize"
))]
#[serde(bound(
    deserialize = "<E as ff::ScalarEngine>::Fr: serde::Deserialize<'de>, <E as pairing::Engine>::G1: serde::Deserialize<'de>, <E as pairing::Engine>::G2: serde::Deserialize<'de>"
))]
pub struct RPSecretParams<E: Engine> {
    pub pubParams: RPPublicParams<E>,
    pub p: SecretParamsUL<E>,
}

impl<E: Engine> SecretParamsUL<E> {
    /**
        setup_ul generates the signature for the interval [0,u^l).
        The value of u should be roughly b/log(b), but we can choose smaller values in
        order to get smaller parameters, at the cost of having worse performance.
    */
    pub fn setup_ul<R: Rng>(rng: &mut R, u: i64, l: i64, csParams: CSMultiParams<E>) -> Self {
        let mpk = setup(rng);
        let kp = BlindKeyPair::<E>::generate(rng, &mpk, 1);

        let mut signatures: HashMap<String, Signature<E>> = HashMap::new();
        for i in 0..u {
            let sig_i = kp.sign(rng, &vec![E::Fr::from_str(i.to_string().as_str()).unwrap()]);
            signatures.insert(i.to_string(), sig_i);
        }
        let pubParams = ParamsUL {
            mpk,
            signatures,
            csParams,
            pk: kp.public.clone(),
            u,
            l,
        };

        SecretParamsUL { pubParams, kp }
    }

    /**
        verify_ul is used to validate the ZKRP proof. It returns true iff the proof is valid.
    */
    pub fn verify_ul(&self, proof: &ProofUL<E>, ch: E::Fr, k: usize) -> bool {
        let r1 = self.verify_part1(&proof, ch.clone(), k);
        let r2 = self.verify_part2(&proof, ch.clone());
        r1 && r2
    }

    fn verify_part2(&self, proof: &ProofUL<E>, challenge: E::Fr) -> bool {
        let mut r2 = true;
        for i in 0..self.pubParams.l as usize {
            let subResult = self.kp.public.verify_proof(
                &self.pubParams.mpk,
                proof.V[i].clone(),
                proof.sigProofs[i].clone(),
                challenge,
            );

            r2 = r2 && subResult;
        }
        r2
    }

    fn verify_part1(&self, proof: &ProofUL<E>, challenge: E::Fr, k: usize) -> bool {
        let mut D = proof.comm.c.clone();
        D.mul_assign(challenge);
        D.negate();
        let mut hzr = self.pubParams.csParams.pub_bases[0].clone();
        hzr.mul_assign(proof.zr);
        D.add_assign(&hzr);
        for i in 0..self.pubParams.l as usize {
            let ui = self.pubParams.u.pow(i as u32);
            let mut aux = self.pubParams.csParams.pub_bases[k].clone();
            for j in 0..self.kp.public.Y1.len() {
                let mut muizsigi = proof.sigProofs[i].zsig[j];
                muizsigi.mul_assign(&E::Fr::from_str(&ui.to_string()).unwrap());
                aux.mul_assign(muizsigi);
            }
            D.add_assign(&aux);
        }
        for i in 1..self.pubParams.csParams.pub_bases.len() {
            let j: usize;
            if i < k {
                j = i - 1;
            } else if i > k {
                j = i - 2;
            } else {
                continue;
            }
            let mut g = self.pubParams.csParams.pub_bases[i].clone();
            g.mul_assign(proof.zs[j].into_repr());
            D.add_assign(&g);
        }
        D == proof.D
    }
}

impl<E: Engine> ParamsUL<E> {
    /**
        prove_ul method is used to produce the ZKRP proof that secret x belongs to the interval [0,U^L).
    */
    pub fn prove_ul<R: Rng>(
        &self,
        rng: &mut R,
        x: i64,
        r: E::Fr,
        C: Commitment<E>,
        k: usize,
        otherM: Vec<E::Fr>,
    ) -> ProofUL<E> {
        let proofUlState = self.prove_ul_commitment(rng, x, k, None, None);

        // Fiat-Shamir heuristic
        let mut a = Vec::<E::Fqk>::with_capacity(self.l as usize);
        for state in proofUlState.proofStates.clone() {
            a.push(state.a);
        }
        let c = hash::<E>(a, vec![proofUlState.D.clone()]);

        self.prove_ul_response(r, C, &proofUlState, c, k, otherM)
    }

    pub fn prove_ul_commitment<R: Rng>(
        &self,
        rng: &mut R,
        x: i64,
        k: usize,
        sOptional: Option<Vec<E::Fr>>,
        mOptional: Option<E::Fr>,
    ) -> ProofULState<E> {
        if x > ((self.u as i128).pow(self.l as u32) - 1) as i64 || x < 0 {
            panic!("x is not within the range.");
        }
        let decx = decompose(x, self.u, self.l);

        // Initialize variables
        let mut proofStates = Vec::<ProofState<E>>::with_capacity(self.l as usize);
        let mut V = Vec::<Signature<E>>::with_capacity(self.l as usize);
        let mut s = Vec::<E::Fr>::with_capacity(self.csParams.pub_bases.len() - 2);
        let mut D = E::G1::zero();
        let m = mOptional.unwrap_or(E::Fr::rand(rng));

        // D = H^m
        let mut hm = self.csParams.pub_bases[0].clone();
        hm.mul_assign(m);
        for i in 0..self.l as usize {
            let signature = self.signatures.get(&decx[i].to_string()).unwrap();
            let proofState = self
                .pk
                .prove_commitment(rng, &self.mpk, &signature, None, None);

            V.push(proofState.blindSig.clone());
            proofStates.push(proofState);

            let ui = self.u.pow(i as u32);
            let mut aux = self.csParams.pub_bases[k].clone();
            for j in 0..self.pk.Y1.len() {
                let mut muiti = proofStates[i].t[j].clone();
                muiti.mul_assign(&E::Fr::from_str(&ui.to_string()).unwrap());
                aux.mul_assign(muiti);
            }
            D.add_assign(&aux);
        }

        let sVec = sOptional.unwrap_or(Vec::<E::Fr>::with_capacity(0));
        for i in 1..self.csParams.pub_bases.len() {
            if i != k {
                let mut g = self.csParams.pub_bases[i].clone();
                let s1: E::Fr;
                if sVec.len() >= i {
                    s1 = sVec[i - 1];
                } else {
                    s1 = E::Fr::rand(rng);
                }
                s.push(s1);
                g.mul_assign(s1);
                D.add_assign(&g);
            }
        }

        D.add_assign(&hm);
        ProofULState {
            decx,
            proofStates,
            V,
            D,
            m,
            s,
        }
    }

    pub fn prove_ul_response(
        &self,
        r: E::Fr,
        C: Commitment<E>,
        proofUlState: &ProofULState<E>,
        c: E::Fr,
        k: usize,
        otherM: Vec<E::Fr>,
    ) -> ProofUL<E> {
        let mut sigProofs = Vec::<SignatureProof<E>>::with_capacity(self.l as usize);
        let mut zr = proofUlState.m.clone();
        let mut rc = r.clone();
        rc.mul_assign(&c);
        zr.add_assign(&rc);
        for i in 0..self.l as usize {
            let dx = E::Fr::from_str(&proofUlState.decx[i].to_string()).unwrap();

            let proof =
                self.pk
                    .prove_response(&proofUlState.proofStates[i].clone(), c, &mut vec![dx]);

            sigProofs.push(proof);
        }

        let mut zs = Vec::<E::Fr>::with_capacity(self.csParams.pub_bases.len() - 2);
        for i in 1..self.csParams.pub_bases.len() {
            let j: usize;
            if i < k {
                j = i - 1;
            } else if i > k {
                j = i - 2;
            } else {
                continue;
            }
            let mut mc = otherM[j].clone();
            mc.mul_assign(&c);
            let mut s = proofUlState.s[j].clone();
            s.add_assign(&mc);
            zs.push(s);
        }
        ProofUL {
            V: proofUlState.V.clone(),
            D: proofUlState.D.clone(),
            comm: C,
            sigProofs,
            zr,
            zs,
        }
    }
}

fn hash<E: Engine>(a: Vec<E::Fqk>, D: Vec<E::G1>) -> E::Fr {
    // create a Sha256 object
    let mut a_vec: Vec<u8> = Vec::new();
    for a_el in a {
        a_vec.extend(format!("{}", a_el).bytes());
    }

    let mut x_vec: Vec<u8> = Vec::new();
    for d_el in D {
        x_vec.extend(format!("{}", d_el).bytes());
    }
    a_vec.extend(x_vec);

    util::hash_to_fr::<E>(a_vec)
}

/*
Decompose receives as input an integer x and outputs an array of integers such that
x = sum(xi.u^i), i.e. it returns the decomposition of x into base u.
*/
fn decompose(x: i64, u: i64, l: i64) -> Vec<i64> {
    let mut result = Vec::with_capacity(l as usize);
    let mut decomposer = x.clone();
    for _i in 0..l {
        result.push(decomposer % u);
        decomposer = decomposer / u;
    }
    return result;
}

impl<E: Engine> RPSecretParams<E> {
    /**
        Setup receives integers a and b, and configures the parameters for the rangeproof scheme.
    */
    pub fn setup<R: Rng>(rng: &mut R, a: i64, b: i64, csParams: CSMultiParams<E>) -> Self {
        // Compute optimal values for u and l
        if a > b {
            panic!("a must be less than or equal to b");
        }

        let logb = (b as f32).log2();
        let loglogb = logb.log2();
        if loglogb > 0.0 {
            //            let mut u = (logb / loglogb) as i64;
            let u = 57; //TODO: optimize u?
            let l = (b as f64).log(u as f64).ceil() as i64;

            let secParamsOut = SecretParamsUL::<E>::setup_ul(rng, u, l, csParams.clone());
            let pubParams = RPPublicParams {
                p: secParamsOut.pubParams.clone(),
                a,
                b,
            };
            RPSecretParams {
                pubParams,
                p: secParamsOut,
            }
        } else {
            panic!("log(log(b)) is zero");
        }
    }

    /**
        Verify is responsible for validating the range proof.
    */
    pub fn verify(&self, proof: RangeProof<E>, ch: E::Fr, k: usize) -> bool {
        let first = self.p.verify_ul(&proof.p1, ch.clone(), k);
        let second = self.p.verify_ul(&proof.p2, ch.clone(), k);
        first & &second
    }

    pub fn compute_challenge(&self, proof: &RangeProof<E>) -> E::Fr {
        let mut a = Vec::<E::Fqk>::with_capacity(self.p.pubParams.l as usize);
        for i in 0..proof.p1.sigProofs.len() {
            a.push(proof.p1.sigProofs[i].a);
            a.push(proof.p2.sigProofs[i].a);
        }
        hash::<E>(a, vec![proof.p1.D.clone(), proof.p2.D.clone()])
    }
}

impl<E: Engine> RPPublicParams<E> {
    /**
        Prove method is responsible for generating the zero knowledge range proof.
    */
    pub fn prove<R: Rng>(
        &self,
        rng: &mut R,
        x: i64,
        C: Commitment<E>,
        r: E::Fr,
        k: usize,
        otherM: Vec<E::Fr>,
    ) -> RangeProof<E> {
        let rpState = self.prove_commitment(rng, x, C, k, None, None);

        let mut a = Vec::<E::Fqk>::with_capacity(self.p.l as usize);
        for i in 0..rpState.ps1.proofStates.len() {
            a.push(rpState.ps1.proofStates[i].a);
            a.push(rpState.ps2.proofStates[i].a);
        }
        let ch = hash::<E>(a, vec![rpState.ps1.D.clone(), rpState.ps2.D.clone()]);

        self.prove_response(r, &rpState, ch, k, otherM)
    }

    pub fn prove_commitment<R: Rng>(
        &self,
        rng: &mut R,
        x: i64,
        C: Commitment<E>,
        k: usize,
        sOptional: Option<Vec<E::Fr>>,
        mOptional: Option<E::Fr>,
    ) -> RangeProofState<E> {
        if x > self.b || x < self.a {
            panic!("x is not within the range.");
        }
        let ul = self.p.u.pow(self.p.l as u32);
        // x - b + ul
        let xb = x - self.b + ul;
        let mut gb = self.p.csParams.pub_bases[k].clone();
        let mut b = E::Fr::from_str(&(self.b.to_string())).unwrap();
        b.negate();
        gb.mul_assign(b.into_repr());
        let mut gul = self.p.csParams.pub_bases[k].clone();
        gul.mul_assign(E::Fr::from_str(&(ul.to_string())).unwrap().into_repr());
        let mut comXB = C.clone();
        comXB.c.add_assign(&gb);
        comXB.c.add_assign(&gul);
        let firstState =
            self.p
                .prove_ul_commitment(rng, xb, k, sOptional.clone(), mOptional.clone());
        // x - a
        let xa = x - self.a;
        let mut ga = self.p.csParams.pub_bases[k].clone();
        let mut a = E::Fr::from_str(&(self.a.to_string())).unwrap();
        a.negate();
        ga.mul_assign(a.into_repr());
        let mut comXA = C.clone();
        comXA.c.add_assign(&ga);
        let secondState =
            self.p
                .prove_ul_commitment(rng, xa, k, sOptional.clone(), mOptional.clone());
        RangeProofState {
            com1: comXB,
            ps1: firstState,
            com2: comXA,
            ps2: secondState,
        }
    }

    pub fn prove_response(
        &self,
        r: E::Fr,
        rpState: &RangeProofState<E>,
        ch: E::Fr,
        k: usize,
        otherM: Vec<E::Fr>,
    ) -> RangeProof<E> {
        let first = self.p.prove_ul_response(
            r.clone(),
            rpState.com1.clone(),
            &rpState.ps1,
            ch.clone(),
            k,
            otherM.clone(),
        );
        let second = self.p.prove_ul_response(
            r.clone(),
            rpState.com2.clone(),
            &rpState.ps2,
            ch.clone(),
            k,
            otherM.clone(),
        );
        RangeProof {
            p1: first,
            p2: second,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pairing::bls12_381::{Bls12, Fr, G1};
    use rand::rngs::ThreadRng;
    use std::mem;
    use std::ops::Add;
    use time::PreciseTime;

    #[test]
    fn setup_ul_works() {
        let rng = &mut rand::thread_rng();
        let csParams = CSMultiParams::setup_gen_params(rng, 1);

        let secParams = SecretParamsUL::<Bls12>::setup_ul(rng, 2, 3, csParams.clone());
        assert_eq!(secParams.pubParams.signatures.len(), 2);
        for (m, s) in secParams.pubParams.signatures {
            assert_eq!(
                secParams.kp.public.verify_blind(
                    &secParams.pubParams.mpk,
                    &vec! {Fr::from_str(m.to_string().as_str()).unwrap()},
                    &Fr::zero(),
                    &s
                ),
                true
            );
        }
    }

    #[test]
    fn prove_ul_works() {
        let rng = &mut rand::thread_rng();
        let csParams = CSMultiParams::setup_gen_params(rng, 1);

        let secParams = SecretParamsUL::<Bls12>::setup_ul(rng, 2, 4, csParams.clone());
        let fr = Fr::rand(rng);
        let modx = Fr::from_str(&(10.to_string())).unwrap();
        let C = csParams.commit(&vec![modx], &fr.clone());
        let proof = secParams.pubParams.prove_ul(rng, 10, fr, C, 1, vec![]);
        assert_eq!(proof.V.len(), 4);
        assert_eq!(proof.sigProofs.len(), 4);
    }

    #[test]
    #[should_panic(expected = "x is not within the range")]
    fn prove_ul_not_in_range() {
        let rng = &mut rand::thread_rng();
        let csParams = CSMultiParams::setup_gen_params(rng, 1);
        let secParams = SecretParamsUL::<Bls12>::setup_ul(rng, 2, 3, csParams.clone());
        let fr = Fr::rand(rng);
        let modx = Fr::from_str(&(100.to_string())).unwrap();
        let C = csParams.commit(&vec![modx], &fr.clone());
        secParams.pubParams.prove_ul(rng, 100, fr, C, 1, vec![]);
    }

    #[test]
    fn prove_and_verify_part1_ul_works() {
        let rng = &mut rand::thread_rng();
        let csParams = CSMultiParams::setup_gen_params(rng, 1);
        let secParams = SecretParamsUL::<Bls12>::setup_ul(rng, 2, 4, csParams.clone());
        let fr = Fr::rand(rng);
        let modx = Fr::from_str(&(10.to_string())).unwrap();
        let C = csParams.commit(&vec![modx], &fr.clone());
        let proof = secParams.pubParams.prove_ul(rng, 10, fr, C, 1, vec![]);
        let ch = compute_challenge(secParams.pubParams.clone(), &proof);
        assert_eq!(secParams.verify_part1(&proof, ch, 1), true);
    }

    #[test]
    fn prove_and_verify_part2_ul_works() {
        let rng = &mut rand::thread_rng();
        let csParams = CSMultiParams::setup_gen_params(rng, 1);
        let secParams = SecretParamsUL::<Bls12>::setup_ul(rng, 2, 4, csParams.clone());
        let fr = Fr::rand(rng);
        let modx = Fr::from_str(&(10.to_string())).unwrap();
        let C = csParams.commit(&vec![modx], &fr.clone());
        let proof = secParams.pubParams.prove_ul(rng, 10, fr, C, 1, vec![]);
        let ch = compute_challenge(secParams.pubParams.clone(), &proof);
        assert_eq!(secParams.verify_part2(&proof, ch), true);
    }

    #[test]
    fn prove_and_verify_ul_works() {
        let rng = &mut rand::thread_rng();
        let csParams = CSMultiParams::setup_gen_params(rng, 1);
        let secParams = SecretParamsUL::<Bls12>::setup_ul(rng, 2, 4, csParams.clone());
        let fr = Fr::rand(rng);
        let modx = Fr::from_str(&(10.to_string())).unwrap();
        let C = csParams.commit(&vec![modx], &fr.clone());
        let proof = secParams.pubParams.prove_ul(rng, 10, fr, C, 1, vec![]);
        let ch = compute_challenge(secParams.pubParams.clone(), &proof);
        assert_eq!(secParams.verify_ul(&proof, ch, 1), true);
    }

    #[test]
    fn prove_and_verify_ul_bigger_commit_works() {
        let rng = &mut rand::thread_rng();
        let csParams = CSMultiParams::setup_gen_params(rng, 3);
        let secParams = SecretParamsUL::<Bls12>::setup_ul(rng, 2, 4, csParams.clone());
        let fr = Fr::rand(rng);
        let modx = Fr::from_str(&(10.to_string())).unwrap();
        let fr1 = Fr::rand(rng);
        let fr2 = Fr::rand(rng);
        let C = csParams.commit(&vec![fr1, modx, fr2], &fr.clone());
        let proof = secParams
            .pubParams
            .prove_ul(rng, 10, fr, C, 2, vec![fr1, fr2]);
        let ch = compute_challenge(secParams.pubParams.clone(), &proof);
        assert_eq!(secParams.verify_ul(&proof, ch, 2), true);
    }

    #[test]
    fn prove_and_verify_works() {
        let rng = &mut rand::thread_rng();
        let csParams = CSMultiParams::setup_gen_params(rng, 1);
        let secParams = RPSecretParams::<Bls12>::setup(rng, 2, 25, csParams.clone());
        let fr = Fr::rand(rng);
        let modx = Fr::from_str(&(10.to_string())).unwrap();
        let C = csParams.commit(&vec![modx], &fr.clone());
        let proof = secParams.pubParams.prove(rng, 10, C, fr, 1, vec![]);
        let ch = secParams.compute_challenge(&proof);

        assert_eq!(secParams.verify(proof, ch, 1), true);
    }

    #[test]
    fn prove_and_verify_bigger_commit_works() {
        let rng = &mut rand::thread_rng();
        let csParams = CSMultiParams::setup_gen_params(rng, 3);
        let secParams = RPSecretParams::<Bls12>::setup(rng, 2, 25, csParams.clone());
        let fr = Fr::rand(rng);
        let modx = Fr::from_str(&(10.to_string())).unwrap();
        let fr1 = Fr::rand(rng);
        let fr2 = Fr::rand(rng);
        let C = csParams.commit(&vec![fr1, modx, fr2], &fr.clone());
        let proof = secParams.pubParams.prove(rng, 10, C, fr, 2, vec![fr1, fr2]);
        let ch = secParams.compute_challenge(&proof);

        assert_eq!(secParams.verify(proof, ch, 2), true);
    }

    #[test]
    #[should_panic(expected = "x is not within the range")]
    fn prove_not_in_range() {
        let rng = &mut rand::thread_rng();
        let csParams = CSMultiParams::setup_gen_params(rng, 1);
        let secParams = RPSecretParams::<Bls12>::setup(rng, 2, 25, csParams.clone());
        let fr = Fr::rand(rng);
        let modx = Fr::from_str(&(26.to_string())).unwrap();
        let C = csParams.commit(&vec![modx], &fr.clone());
        secParams.pubParams.prove(rng, 26, C, fr, 1, vec![]);
    }

    #[test]
    #[ignore]
    fn prove_and_verify_performance() {
        let rng = &mut rand::thread_rng();
        let mut averageSetup = time::Duration::nanoseconds(0);
        let mut averageSetupSize = 0;
        let mut averageProve = time::Duration::nanoseconds(0);
        let mut averageProofSize = 0;
        let mut averageVerify = time::Duration::nanoseconds(0);
        let iter = 5;
        for _i in 0..iter {
            let a = rng.gen_range(0, 1000000);
            let b = rng.gen_range(a, 1000000);
            let x = rng.gen_range(a, b);

            let sSetup = PreciseTime::now();
            let csParams = CSMultiParams::setup_gen_params(rng, 1);
            let secParams = RPSecretParams::<Bls12>::setup(rng, a, b, csParams.clone());
            averageSetup = averageSetup.add(sSetup.to(PreciseTime::now()));
            averageSetupSize += mem::size_of_val(&secParams.pubParams);

            let sProve = PreciseTime::now();
            let fr = Fr::rand(rng);
            let modx = Fr::from_str(&(x.to_string())).unwrap();
            let C = csParams.commit(&vec![modx], &fr.clone());
            let proof = secParams.pubParams.prove(rng, x, C, fr, 1, vec![]);
            averageProve = averageProve.add(sProve.to(PreciseTime::now()));
            averageProofSize += mem::size_of_val(&proof);

            let sVerify = PreciseTime::now();
            let ch = secParams.compute_challenge(&proof);
            secParams.verify(proof, ch, 1);
            averageVerify = averageVerify.add(sVerify.to(PreciseTime::now()));
        }
        print!("Setup: {}\n", averageSetup.num_milliseconds() / iter);
        print!("Setup size: {}\n", averageSetupSize / iter as usize);
        print!("Prove: {}\n", averageProve.num_milliseconds() / iter);
        print!("Proof size: {}\n", averageProofSize / iter as usize);
        print!("Verify: {}\n", averageVerify.num_milliseconds() / iter);
    }

    #[test]
    fn decompose_works() {
        assert_eq!(decompose(25, 3, 3), vec! {1, 2, 2});
        assert_eq!(decompose(336, 7, 3), vec! {0, 6, 6});
        assert_eq!(decompose(285, 8, 3), vec! {5, 3, 4});
        assert_eq!(decompose(125, 13, 2), vec! {8, 9});
        assert_eq!(decompose(143225, 6, 7), vec! {5, 2, 0, 3, 2, 0, 3});
    }

    #[test]
    fn decompose_recompose_works() {
        let vec1 = decompose(25, 3, 5);
        let mut result = 0;
        for i in 0..5 {
            result += vec1[i] * 3i64.pow(i as u32);
        }
        assert_eq!(result, 25);

        let vec1 = decompose(143225, 6, 7);
        let mut result = 0;
        for i in 0..7 {
            result += vec1[i] * 6i64.pow(i as u32);
        }
        assert_eq!(result, 143225);
    }

    #[test]
    fn setup_works() {
        let rng = &mut rand::thread_rng();
        let csParams = CSMultiParams::setup_gen_params(rng, 1);
        let secParams = RPSecretParams::<Bls12>::setup(rng, 2, 10, csParams);
        let public_params = secParams.pubParams.clone();
        assert_eq!(public_params.a, 2);
        assert_eq!(public_params.b, 10);
        assert_eq!(public_params.p.signatures.len(), 57);
        assert_eq!(public_params.p.u, 57);
        assert_eq!(public_params.p.l, 1);
        for (m, s) in public_params.p.signatures {
            assert_eq!(
                secParams.p.kp.public.verify_blind(
                    &public_params.p.mpk,
                    &vec! {Fr::from_str(m.to_string().as_str()).unwrap()},
                    &Fr::zero(),
                    &s
                ),
                true
            );
        }
    }

    #[test]
    #[should_panic(expected = "a must be less than or equal to b")]
    fn setup_wrong_a_and_b() {
        let rng = &mut rand::thread_rng();
        let csParams = CSMultiParams::setup_gen_params(rng, 1);
        RPSecretParams::<Bls12>::setup(rng, 10, 2, csParams);
    }

    #[test]
    #[should_panic(expected = "log(log(b)) is zero")]
    fn setup_wrong_logb() {
        let rng = &mut rand::thread_rng();
        let csParams = CSMultiParams::setup_gen_params(rng, 1);
        RPSecretParams::<Bls12>::setup(rng, -2, -1, csParams);
    }

    #[test]
    fn hash_works() {
        let rng = &mut rand::thread_rng();
        let D = G1::rand(rng);
        let D2 = G1::rand(rng);
        let params = setup::<ThreadRng, Bls12>(rng);
        let kp = BlindKeyPair::generate(rng, &params, 2);
        let m1 = Fr::rand(rng);
        let m2 = Fr::rand(rng);
        let sig = kp.sign(rng, &vec![m1, m2]);
        let state = kp.public.prove_commitment(rng, &params, &sig, None, None);
        let state1 = kp.public.prove_commitment(rng, &params, &sig, None, None);
        let state2 = kp.public.prove_commitment(rng, &params, &sig, None, None);
        let state3 = kp.public.prove_commitment(rng, &params, &sig, None, None);
        let state4 = kp.public.prove_commitment(rng, &params, &sig, None, None);
        let a = vec![state.a, state1.a, state2.a];
        let a2 = vec![state3.a, state4.a];
        assert_eq!(hash::<Bls12>(a.clone(), vec!(D.clone())).is_zero(), false);
        assert_ne!(
            hash::<Bls12>(a2.clone(), vec!(D.clone())),
            hash::<Bls12>(a.clone(), vec!(D.clone()))
        );
        assert_ne!(
            hash::<Bls12>(a.clone(), vec!(D2.clone())),
            hash::<Bls12>(a.clone(), vec!(D.clone()))
        );
        assert_ne!(
            hash::<Bls12>(a2.clone(), vec!(D2.clone())),
            hash::<Bls12>(a.clone(), vec!(D.clone()))
        )
    }

    fn compute_challenge<E: Engine>(pubParams: ParamsUL<E>, proof: &ProofUL<E>) -> E::Fr {
        let mut a = Vec::<E::Fqk>::with_capacity(pubParams.l as usize);
        for sigProof in proof.sigProofs.clone() {
            a.push(sigProof.a);
        }
        hash::<E>(a, vec![proof.D.clone()])
    }
}
