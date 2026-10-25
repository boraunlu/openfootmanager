//! Narrow host boundary: native private valuations and native agreed execution.
use super::*;

pub fn transfer_negotiation_limits(
    game: &Game,
    player_id: &str,
    buyer_id: &str,
    seller_id: &str,
    opening_fee: u64,
) -> Result<(u64, u64), String> {
    let player = game
        .players
        .iter()
        .find(|p| p.id == player_id && !p.retired && p.team_id.as_deref() == Some(seller_id))
        .ok_or("be.error.playerNotFound")?;
    let buyer = game
        .teams
        .iter()
        .find(|t| t.id == buyer_id)
        .ok_or("be.error.teamNotFound")?;
    let seller = game
        .teams
        .iter()
        .find(|t| t.id == seller_id)
        .ok_or("be.error.teamNotFound")?;
    let day = game.clock.current_date.date_naive();
    Ok((
        minimum_acceptable_fee(day, player, seller, buyer),
        buyer_counter_offer_ceiling(day, player, opening_fee, buyer),
    ))
}

pub fn complete_negotiated_transfer(
    game: &mut Game,
    player_id: &str,
    buyer_id: &str,
    seller_id: &str,
    offer_id: &str,
    fee: u64,
) -> Result<(), String> {
    if buyer_id == seller_id {
        return Err(ERR_CANNOT_BID_ON_OWN_PLAYER.into());
    }
    let player = game
        .players
        .iter()
        .find(|p| p.id == player_id && !p.retired && p.team_id.as_deref() == Some(seller_id))
        .ok_or("be.error.playerNotFound")?;
    if player_has_active_or_pending_loan(player) || has_pending_transfer_registration(player) {
        return Err(ERR_PLAYER_ALREADY_LOANED.into());
    }
    if fee > i64::MAX as u64 || !transfer_buyer_can_register(game, buyer_id, fee) {
        return Err(ERR_INSUFFICIENT_FUNDS.into());
    }
    let seller = game
        .teams
        .iter()
        .find(|t| t.id == seller_id)
        .ok_or("be.error.teamNotFound")?;
    if seller.finance.checked_add(fee as i64).is_none()
        || seller.transfer_budget.checked_add(fee as i64).is_none()
    {
        return Err("be.error.transfers.insufficientFunds".into());
    }
    if !player.transfer_offers.iter().any(|o| {
        o.id == offer_id && o.from_team_id == buyer_id && o.status == TransferOfferStatus::Pending
    }) {
        return Err(ERR_OFFER_NOT_PENDING.into());
    }
    execute_transfer(game, player_id, buyer_id, seller_id, fee)?;
    finalize_successful_transfer_offer(game, player_id, offer_id)?;
    if let Some(offer) = game
        .players
        .iter_mut()
        .find(|p| p.id == player_id)
        .and_then(|p| p.transfer_offers.iter_mut().find(|o| o.id == offer_id))
    {
        offer.fee = fee;
        offer.status = TransferOfferStatus::Accepted;
        offer.suggested_counter_fee = None;
        offer.registration_date = None;
    }
    Ok(())
}
