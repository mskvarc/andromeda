//! Updating the reduced copy a context source holds (clause 5.8.1.4,
//! "updated"), and what the source made of the update.
//!
//! An update carries the whole reduction, the notification endpoint included,
//! so a copy its source took names the receiver this broker serves, and the
//! redirect ledger records it as owing nothing.

use crate::{
    error::SubscriptionForwardingError,
    forwarder::CopyForm,
    redirect_ledger::RedirectTarget,
    reduced_copy::ReducedSubscription,
    service::SubscriptionForwardingService,
};
use andromeda_federation::{
    answer_verdict::AnswerVerdict,
    subscription::{ForwardedSubscription, SubscriptionForward, SubscriptionForwardAdmission},
    subscription_forward_failure::SubscriptionForwardFailure,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{registration::ContextSourceRegistration, subscription::Subscription};
use andromeda_subscription::{deleted_members::DeletedMembers, forwarding_hook::ForwardedCopy};

/// What a source made of an update of the copy it holds.
#[derive(Debug)]
pub(crate) enum CopyUpdate {
    /// The source took the update, so the copy names the current receiver.
    Taken,
    /// The registration does not advertise the update of subscriptions, so
    /// nothing was sent.
    NotAdvertised,
    /// The source never takes the update as sent, because it refused it as
    /// sent or the registration loops back into this broker.
    Refused(SubscriptionForwardFailure),
    /// No verdict came back, or one that depends on the moment the update
    /// came, so the source may take it another time.
    Withheld(SubscriptionForwardFailure),
}

impl CopyUpdate {
    /// What `forwarded`, the forward of an update, says the source made of it.
    fn of(forwarded: &ForwardedSubscription) -> CopyUpdate {
        if forwarded.admission == SubscriptionForwardAdmission::NotAdvertised {
            return CopyUpdate::NotAdvertised;
        }
        let Some(failure) = forwarded.failure() else {
            return CopyUpdate::Taken;
        };
        let verdict = match &failure {
            SubscriptionForwardFailure::Answered(answer) => answer.verdict(),
            SubscriptionForwardFailure::Transport(_) => AnswerVerdict::Withheld,
            SubscriptionForwardFailure::LoopDetected => AnswerVerdict::Refused,
            SubscriptionForwardFailure::NotAdvertised => return CopyUpdate::NotAdvertised,
        };
        match verdict {
            AnswerVerdict::Refused => CopyUpdate::Refused(failure),
            AnswerVerdict::Withheld => CopyUpdate::Withheld(failure),
        }
    }
}

impl SubscriptionForwardingService {
    /// Update the copy of `subscription` that `registration`'s source holds
    /// under `remote` to the subscription as it stands, written in `form`,
    /// sending each member in `deleted` the copy still holds as a deletion.
    ///
    /// A copy the source took is recorded as naming the current receiver.
    ///
    /// # Errors
    ///
    /// Propagates the failure to process the subscription's `@context`, to
    /// reduce the subscription and to record the copy in the redirect ledger.
    pub(crate) async fn update_copy(
        &self,
        subscription: &Subscription,
        registration: &ContextSourceRegistration,
        remote: &Identifier,
        form: &CopyForm,
        deleted: &DeletedMembers,
    ) -> Result<CopyUpdate, SubscriptionForwardingError> {
        let names = self.names_of(&form.context).await?;
        // Boxed: the fragment is held across the forward, and the request is a wide value.
        let fragment = Box::new(
            ReducedSubscription::reduce(subscription, registration, &self.receiver, remote, form.carriage, &names)
                .await?
                .as_fragment(deleted, form.carriage),
        );
        let forwarded = self
            .federation
            .forward_subscription(
                registration,
                SubscriptionForward::Update {
                    remote_id: remote,
                    body: &fragment,
                    context: &form.context,
                },
            )
            .await;
        let update = CopyUpdate::of(&forwarded);
        match update {
            CopyUpdate::Taken => {
                let copy = RedirectTarget::Copy(ForwardedCopy {
                    registration: registration.id.clone(),
                    remote_subscription: remote.clone(),
                });
                self.redirects.redirected(&subscription.id, &copy).await?;
            }
            CopyUpdate::NotAdvertised | CopyUpdate::Refused(_) | CopyUpdate::Withheld(_) => {}
        }
        Ok(update)
    }
}

#[cfg(test)]
mod tests {
    use crate::copy_update::CopyUpdate;
    use andromeda_federation::{
        answer_body::AnswerBody,
        outcome::ForwardedWriteOutcome,
        subscription::{ForwardedSubscription, SubscriptionForwardAdmission},
    };

    fn forwarded(outcome: ForwardedWriteOutcome) -> ForwardedSubscription {
        ForwardedSubscription {
            admission: SubscriptionForwardAdmission::Forwarded,
            outcome: Some(outcome),
        }
    }

    fn answered(status: u16) -> ForwardedSubscription {
        forwarded(ForwardedWriteOutcome::RemoteHttpFailure {
            status,
            body: AnswerBody::Absent,
        })
    }

    #[test]
    fn a_completed_update_is_taken() {
        let completed = forwarded(ForwardedWriteOutcome::Completed {
            status: 204,
            body: AnswerBody::Absent,
        });
        assert!(matches!(CopyUpdate::of(&completed), CopyUpdate::Taken));
    }

    #[test]
    fn an_update_never_sent_is_not_advertised() {
        let unsent = ForwardedSubscription {
            admission: SubscriptionForwardAdmission::NotAdvertised,
            outcome: None,
        };
        assert!(matches!(CopyUpdate::of(&unsent), CopyUpdate::NotAdvertised));
    }

    /// A 404 says the source holds no copy under the identifier, and a 400
    /// refuses the fragment as sent, so neither is taken another time.
    #[test]
    fn an_update_the_source_refused_as_sent_is_refused() {
        for status in [400, 404] {
            assert!(matches!(CopyUpdate::of(&answered(status)), CopyUpdate::Refused(_)), "{status}");
        }
        assert!(matches!(
            CopyUpdate::of(&forwarded(ForwardedWriteOutcome::LoopDetected)),
            CopyUpdate::Refused(_)
        ));
    }

    #[test]
    fn an_update_answered_by_the_moment_it_came_is_withheld() {
        for status in [429, 500, 503, 504] {
            assert!(matches!(CopyUpdate::of(&answered(status)), CopyUpdate::Withheld(_)), "{status}");
        }
    }
}
