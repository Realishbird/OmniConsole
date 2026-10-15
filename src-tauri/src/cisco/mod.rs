//! Cisco-specific knowledge: which OS is on the other end, how to read its
//! `show` output, and what healthy looks like.

pub mod checks;
pub mod classify;
pub mod configs;
pub mod firmware;
pub mod generic;
pub mod ise;
pub mod templates;
pub mod wireless;
pub mod detect;
pub mod parse;
