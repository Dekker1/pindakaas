use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use itertools::Itertools;

pub(crate) use crate::int::IntVar;
use crate::{
	int::{Consistency, IntVarEnc, Lin, Model},
	linear::LimitComp,
	ClauseDatabase, Coeff, Encoder, Linear, Result,
};

const EQUALIZE_INTERMEDIATES: bool = false;

/// Encode the constraint that ∑ coeffᵢ·litsᵢ ≦ k using a Generalized Totalizer (GT)
#[derive(Clone, Default)]
pub struct TotalizerEncoder {
	add_consistency: bool,
	add_propagation: Consistency,
	cutoff: Option<Coeff>,
}

impl TotalizerEncoder {
	pub fn add_consistency(&mut self, b: bool) -> &mut Self {
		self.add_consistency = b;
		self
	}
	pub fn add_propagation(&mut self, c: Consistency) -> &mut Self {
		self.add_propagation = c;
		self
	}
	pub fn add_cutoff(&mut self, c: Option<Coeff>) -> &mut Self {
		self.cutoff = c;
		self
	}
}

impl<DB: ClauseDatabase> Encoder<DB, Linear> for TotalizerEncoder {
	#[cfg_attr(
		feature = "trace",
		tracing::instrument(name = "totalizer_encoder", skip_all, fields(constraint = lin.trace_print()))
	)]
	fn encode(&self, db: &mut DB, lin: &Linear) -> Result {
		let xs = lin
			.terms
			.iter()
			.enumerate()
			.flat_map(|(i, part)| IntVarEnc::from_part(db, part, lin.k, format!("x_{i}")))
			.sorted_by_key(|x| x.ub())
			.collect_vec();

		// The totalizer encoding constructs a binary tree starting from a layer of leaves
		let mut model = self.build_totalizer(xs, &lin.cmp, *lin.k);
		model.propagate(&self.add_propagation, vec![model.cons.len() - 1]);
		model.encode(db, self.cutoff)
	}
}

impl TotalizerEncoder {
	fn build_totalizer(&self, xs: Vec<IntVarEnc>, cmp: &LimitComp, k: Coeff) -> Model {
		let mut model = Model::default();
		let mut layer = xs
			.into_iter()
			.map(|x| Rc::new(RefCell::new(model.add_int_var_enc(x))))
			.collect_vec();

		while layer.len() > 1 {
			let mut next_layer = Vec::<Rc<RefCell<IntVar>>>::new();
			for children in layer.chunks(2) {
				match children {
					[x] => {
						next_layer.push(x.clone());
					}
					[left, right] => {
						let at_root = layer.len() == 2;
						let dom = if at_root {
							BTreeSet::from([k])
						} else {
							left.borrow()
								.dom
								.iter()
								.cartesian_product(right.borrow().dom.iter())
								.map(|(&a, &b)| a + b)
								.filter(|&d| d <= k)
								.sorted()
								.dedup()
								.collect()
						};
						let parent =
							Rc::new(RefCell::new(model.new_var(dom, self.add_consistency)));

						model.cons.push(Lin::tern(
							left.clone(),
							right.clone(),
							if !at_root && EQUALIZE_INTERMEDIATES {
								LimitComp::Equal
							} else {
								cmp.clone()
							},
							parent.clone(),
						));
						next_layer.push(parent);
					}
					_ => panic!(),
				}
			}
			layer = next_layer;
		}

		model
	}
}

#[cfg(test)]
mod tests {
	#[cfg(feature = "trace")]
	use traced_test::test;

	use super::*;
	use crate::{
		// cardinality_one::tests::card1_test_suite, CardinalityOne,
		helpers::tests::{assert_sol, lits, TestDB},
		linear::{
			tests::{construct_terms, linear_test_suite},
			LimitComp, PosCoeff,
		},
		Comparator,
		Encoder,
		LinExp,
		LinearAggregator,
		LinearConstraint,
		Lit,
		SortedEncoder,
	};

	#[test]
	fn test_sort_same_coefficients_2() {
		use crate::{
			linear::{LinearEncoder, StaticLinEncoder},
			Checker, Encoder,
		};
		let mut db = TestDB::new(5);
		let mut agg = LinearAggregator::default();
		agg.sort_same_coefficients(SortedEncoder::default(), 3);
		let mut encoder = LinearEncoder::<StaticLinEncoder<TotalizerEncoder>>::default();
		encoder.add_linear_aggregater(agg);
		let con = LinearConstraint::new(
			LinExp::from_slices(&[3, 3, 1, 1, 3], &lits![1, 2, 3, 4, 5]),
			Comparator::GreaterEq,
			2,
		);
		assert!(encoder.encode(&mut db, &con).is_ok());
		db.with_check(|value| {
			LinearConstraint::new(
				LinExp::from_slices(&[3, 3, 1, 1, 3], &lits![1, 2, 3, 4, 5]),
				Comparator::GreaterEq,
				2,
			)
			.check(value)
			.is_ok()
		})
		.check_complete()
	}

	linear_test_suite!(TotalizerEncoder::default());
	// FIXME: Totalizer does not support LimitComp::Equal
	// card1_test_suite!(TotalizerEncoder::default());
}
