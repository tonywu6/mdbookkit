use std::cmp::Ordering;

pub trait LexicographicOrd {
    fn head(&self) -> impl Ord;

    fn tail(&self) -> impl Iterator<Item = impl Ord> {
        std::iter::empty::<()>()
    }
}

pub struct Lexicographic<T>(pub T);

impl<T: LexicographicOrd> Ord for Lexicographic<T> {
    #[inline]
    fn cmp(&self, that: &Self) -> Ordering {
        match self.0.head().cmp(&that.0.head()) {
            Ordering::Equal => {}
            ordering => return ordering,
        }
        let mut lhs = self.0.tail();
        let mut rhs = that.0.tail();
        while let (Some(lhs), Some(rhs)) = (lhs.next(), rhs.next()) {
            match lhs.cmp(&rhs) {
                Ordering::Equal => {}
                ordering => return ordering,
            }
        }
        match (lhs.next(), rhs.next()) {
            (None, Some(_)) => Ordering::Less,
            (None, None) => Ordering::Equal,
            (Some(_), None) => Ordering::Greater,
            (Some(_), Some(_)) => unreachable!(),
        }
    }
}

impl<T: LexicographicOrd> PartialOrd for Lexicographic<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: LexicographicOrd> Eq for Lexicographic<T> {}

impl<T: LexicographicOrd> PartialEq for Lexicographic<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

#[macro_export]
macro_rules! lexicographic_ordering {
    ( $type:ty ) => {
        impl Ord for $type {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                $crate::cmp::Lexicographic(self).cmp(&$crate::cmp::Lexicographic(other))
            }
        }

        impl PartialOrd for $type {
            #[inline]
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Eq for $type {}

        impl PartialEq for $type {
            fn eq(&self, other: &Self) -> bool {
                self.cmp(other).is_eq()
            }
        }
    };
}
