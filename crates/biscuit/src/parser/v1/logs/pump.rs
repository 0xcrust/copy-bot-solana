use crate::parser::{
    generated::pump::types::{CompleteEvent, CreateEvent, SetParamsEvent, TradeEvent},
    v1::split_discriminator,
};
use borsh::BorshDeserialize;

const CREATE_EVENT_DISCRIMINATOR: [u8; 8] = [27, 114, 169, 77, 222, 235, 99, 118];
const TRADE_EVENT_DISCRIMINATOR: [u8; 8] = [189, 219, 127, 211, 78, 230, 97, 238];
const COMPLETE_EVENT_DISCRIMINATOR: [u8; 8] = [95, 114, 97, 156, 212, 46, 152, 8];
const SET_PARAMS_EVENT_DISCRIMINATOR: [u8; 8] = [223, 195, 159, 246, 62, 48, 143, 131];

#[derive(Debug)]
pub enum PumpEvent {
    Create(CreateEvent),
    BondingCurve(CompleteEvent),
    Trade(TradeEvent),
    SetParams(SetParamsEvent),
}

pub fn parse_pumpfun_events(logs: &Vec<Vec<u8>>) -> Vec<PumpEvent> {
    let mut parsed = Vec::with_capacity(logs.len());
    for byte_vec in logs {
        let (discriminator, mut rest) = split_discriminator(&byte_vec);
        match discriminator {
            CREATE_EVENT_DISCRIMINATOR => match CreateEvent::deserialize(&mut rest) {
                Ok(event) => parsed.push(PumpEvent::Create(event)),
                Err(e) => {
                    log::error!("Failed to parse pump `create` event")
                }
            },
            TRADE_EVENT_DISCRIMINATOR => match TradeEvent::deserialize(&mut rest) {
                Ok(event) => parsed.push(PumpEvent::Trade(event)),
                Err(e) => {
                    log::error!("Failed to parse pump `trade` event")
                }
            },
            COMPLETE_EVENT_DISCRIMINATOR => match CompleteEvent::deserialize(&mut rest) {
                Ok(event) => parsed.push(PumpEvent::BondingCurve(event)),
                Err(e) => {
                    log::error!("Failed to parse pump `complete` event")
                }
            },
            SET_PARAMS_EVENT_DISCRIMINATOR => match SetParamsEvent::deserialize(&mut rest) {
                Ok(event) => parsed.push(PumpEvent::SetParams(event)),
                Err(e) => {
                    log::error!("Failed to parse pump `setParams` event")
                }
            },
            _ => {
                log::error!(
                    "Got unknown discriminator for Pump event: {:?}",
                    discriminator
                )
            }
        }
    }
    parsed
}

pub fn parse_pumpfun_swap_events(logs: &Vec<Vec<u8>>) -> Vec<TradeEvent> {
    let mut parsed = Vec::with_capacity(logs.len());
    for byte_vec in logs {
        let (discriminator, mut rest) = split_discriminator(&byte_vec);
        match discriminator {
            TRADE_EVENT_DISCRIMINATOR => match TradeEvent::deserialize(&mut rest) {
                Ok(event) => parsed.push(event),
                Err(e) => {
                    log::error!("Failed to parse pump `trade` event")
                }
            },
            _ => {}
        }
    }
    parsed
}
