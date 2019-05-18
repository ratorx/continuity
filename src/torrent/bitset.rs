use bitvec::BitVec;

pub fn union(bv1: &BitVec, bv2: &BitVec) -> BitVec {
    let ret = bv1.clone();
    ret | bv2
}

pub fn difference(bv1: &BitVec, bv2: &BitVec) -> BitVec {
    let ret = !(bv2.clone());
    ret & bv1
}

pub fn intersect(bv1: &BitVec, bv2: &BitVec) -> BitVec {
    let ret = bv2.clone();
    ret & bv1
}
