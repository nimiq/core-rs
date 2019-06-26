use std::sync::Arc;

use consensus::{ConsensusProtocol, Consensus};
use crate::error::ClientError;
use block_albatross::PbftProposal;


pub trait BlockProducer<P: ConsensusProtocol + 'static>: Sized + Send + Sync {
    type Config: Clone + Sized + Send + Sync;

    fn new(config: Self::Config, consensus: Arc<Consensus<P>>) -> Result<Self, ClientError>;

    fn get_pbft_proposal(&self) -> Option<PbftProposal>;

    fn get_pbft_votes(&self) -> Option<(usize, usize)>;
}


pub struct DummyBlockProducer {}
impl<P: ConsensusProtocol + 'static> BlockProducer<P> for DummyBlockProducer {
    type Config = ();

    fn new(_config: (), _consensus: Arc<Consensus<P>>) -> Result<Self, ClientError> {
        Ok(DummyBlockProducer{})
    }

    fn get_pbft_proposal(&self) -> Option<PbftProposal> {
        None
    }

    fn get_pbft_votes(&self) -> Option<(usize, usize)> {
        None
    }
}



#[cfg(feature = "validator")]
pub mod albatross {
    use std::sync::Arc;

    use consensus::{AlbatrossConsensusProtocol, Consensus};
    use validator::validator::Validator;
    use validator::error::Error as ValidatorError;
    use bls::bls12_381::KeyPair;
    use block_albatross::PbftProposal;

    use super::BlockProducer;
    use crate::error::ClientError;

    #[derive(Clone)]
    pub struct ValidatorConfig {
        pub validator_key: KeyPair,
        pub block_delay: u64,
    }

    pub struct AlbatrossBlockProducer {
        validator: Arc<Validator>,
    }

    impl BlockProducer<AlbatrossConsensusProtocol> for AlbatrossBlockProducer {
        type Config = ValidatorConfig;

        fn new(config: Self::Config, consensus: Arc<Consensus<AlbatrossConsensusProtocol>>) -> Result<Self, ClientError> {
            Ok(Self {
                validator: Validator::new(consensus, config.validator_key, config.block_delay)?
            })
        }

        fn get_pbft_proposal(&self) -> Option<PbftProposal> {
            self.validator.get_pbft_proposal()
        }

        fn get_pbft_votes(&self) -> Option<(usize, usize)> {
            self.validator.get_pbft_votes()
        }
    }

    impl From<ValidatorError> for ClientError {
        fn from(_e: ValidatorError) -> Self {
            ClientError::BlockProducerError
        }
    }
}



#[cfg(feature = "mock-validator")]
pub mod mock {
    use std::sync::Arc;

    use consensus::{AlbatrossConsensusProtocol, Consensus};
    use validator::mock::MockValidator;
    use block_albatross::PbftProposal;

    use super::BlockProducer;
    use crate::error::ClientError;

    #[derive(Clone)]
    pub struct MockBlockProducer {
        validator: Arc<MockValidator>,
    }

    impl BlockProducer<AlbatrossConsensusProtocol> for MockBlockProducer {
        type Config = ();

        fn new(_config: Self::Config, consensus: Arc<Consensus<AlbatrossConsensusProtocol>>) -> Result<Self, ClientError> {
            let validator = MockValidator::new(consensus);
            validator.start();
            Ok(Self {
                validator,
            })
        }

        fn get_pbft_proposal(&self) -> Option<PbftProposal> {
            None
        }

        fn get_pbft_votes(&self) -> Option<(usize, usize)> {
            None
        }
    }
}
