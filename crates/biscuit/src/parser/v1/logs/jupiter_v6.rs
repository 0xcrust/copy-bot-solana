use crate::parser::{
    generated::jupiter_aggregator_v6::types::{FeeEvent, SwapEvent},
    v1::split_discriminator,
};
use borsh::BorshDeserialize;

const SWAP_EVENT_DISCRIMINATOR: [u8; 8] = [64, 198, 205, 232, 38, 8, 113, 226];
const FEE_EVENT_DISCRIMINATOR: [u8; 8] = [81, 108, 227, 190, 205, 208, 10, 196];

pub const EVENT_IX_TAG_LE: [u8; 8] = [228, 69, 165, 46, 81, 203, 154, 29];

#[derive(Debug)]
pub enum JupiterEvent {
    Fee(FeeEvent),
    Swap(SwapEvent),
}

pub fn parse_jupiter_events(logs: &Vec<Vec<u8>>) -> Vec<JupiterEvent> {
    let mut parsed = Vec::with_capacity(logs.len());
    for byte_vec in logs {
        let (discriminator, mut rest) = split_discriminator(&byte_vec);
        if discriminator == FEE_EVENT_DISCRIMINATOR {
            match FeeEvent::deserialize(&mut rest) {
                Ok(event) => parsed.push(JupiterEvent::Fee(event)),
                Err(e) => {
                    log::error!("Failed to parse Jupiter `fee` event")
                }
            }
        } else if discriminator == SWAP_EVENT_DISCRIMINATOR {
            match SwapEvent::deserialize(&mut rest) {
                Ok(event) => parsed.push(JupiterEvent::Swap(event)),
                Err(e) => {
                    log::error!("Failed to parse Jupiter `swap` event")
                }
            }
        } else {
            log::error!(
                "Got unknown discriminator for Jupiter event: {:?}",
                discriminator
            );
        }
    }
    parsed
}

pub fn parse_jupiter_swap_events(logs: &Vec<Vec<u8>>) -> Vec<SwapEvent> {
    let mut parsed = Vec::with_capacity(logs.len());

    for data in logs {
        let (discriminator, mut rest) = split_discriminator(&data);
        match discriminator {
            SWAP_EVENT_DISCRIMINATOR => match SwapEvent::deserialize(&mut rest) {
                Ok(event) => parsed.push(event),
                Err(e) => {
                    log::error!("Failed to parse Jupiter `swap` event")
                }
            },
            _ => {}
        }
    }
    parsed
}

pub fn merge_jupiter_events(events: Vec<SwapEvent>) -> Option<SwapEvent> {
    let mut first = events.first()?.clone();
    let last = events.last()?;

    first.output_mint = last.output_mint;
    first.output_amount = last.output_amount;

    Some(first.clone())
}
