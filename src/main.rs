#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate serde_json;
extern crate lambda_http;
extern crate lambda_runtime as runtime;
extern crate cf_dist_utils;
extern crate cf_functions;
extern crate fang_oost;
extern crate loan_ec;
extern crate num_complex;
extern crate rayon;
use self::num_complex::Complex;
use self::rayon::prelude::*;
use lambda_http::{lambda, IntoResponse, Request, Body, Response};
use runtime::{error::HandlerError, Context};
use std::error::Error;

fn build_response(code: u16, body: &str) -> impl IntoResponse {
    Response::builder()
        .status(code)
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Credentials", "true")
        .body::<Body>(body.into())
        .unwrap()
}
fn construct_error(e_message: &str) -> String {
    json!({ "err": e_message }).to_string()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Parameters {
    lambda: f64,
    q: f64,
    num_u: usize,
    pd: f64, 
    num_loans:f64,
    volatility: f64
}

#[derive(Debug, Serialize)]
struct Element {
    density: f64,
    at_point: f64
}

const NUM_X:usize=512;
const X_MAX:f64=0.0;

fn main() -> Result<(), Box<dyn Error>> {
    lambda!(credit_faas_wrapper);
    Ok(())
}
fn credit_faas_wrapper(event: Request, _ctx: Context) -> Result<impl IntoResponse, HandlerError> {
    match credit_faas(event) {
        Ok(res) => Ok(build_response(200, &json!(res).to_string())),
        Err(e) => Ok(build_response(
            400,
            &construct_error(&e.to_string()),
        )),
    }
}

fn lgd_fn(u: &Complex<f64>, l: f64, _lgd_v: f64)->Complex<f64>{
    (-u*l).exp()
}
fn gamma_mgf(variance: f64) -> impl Fn(&[Complex<f64>]) -> Complex<f64> {
    move |u_weights: &[Complex<f64>]| -> Complex<f64> {
        u_weights
            .iter()
            .map(|u| -(1.0 - variance * u).ln() / variance)
            .sum::<Complex<f64>>()
            .exp()
    }
}

fn credit_faas(event: Request) -> Result<Vec<Element>, Box<dyn Error>> {
    let parameters: Parameters  = serde_json::from_reader(event.body().as_ref())?;
    let Parameters {
        lambda, q, num_u, num_loans,
        pd, volatility 
    }=parameters;
    let x_min=-(1.0+lambda*q)*pd*num_loans*(1.0+volatility)*4.0;
    let liquid_fn = loan_ec::get_liquidity_risk_fn(lambda, q);
    let log_lpm_cf = loan_ec::get_log_lpm_cf(&lgd_fn, &liquid_fn);
    let mut discrete_cf = loan_ec::EconomicCapitalAttributes::new(num_u, 1);
    let u_domain: Vec<Complex<f64>> = fang_oost::get_u_domain(num_u, x_min, 0.0).collect();
    let loan=loan_ec::Loan {
        balance:1.0,
        pd,
        lgd:1.0,
        weight:vec![1.0],
        r: 0.5,
        lgd_variance:0.0,
        num:num_loans
    };
    discrete_cf.process_loan(&loan, &u_domain, &log_lpm_cf);
    let v_mgf = gamma_mgf(volatility.powi(2));
    let final_cf: Vec<Complex<f64>> = discrete_cf.get_full_cf(&v_mgf);
    let result=fang_oost::get_density(x_min, X_MAX, fang_oost::get_x_domain(NUM_X, x_min, X_MAX), &final_cf)
        .zip(fang_oost::get_x_domain(NUM_X, x_min, X_MAX))
        .map(|(density, at_point)|{
            Element{
                density,
                at_point
            }
        }).collect();
    Ok(result)
}