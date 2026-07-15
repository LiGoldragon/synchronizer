//! Render the authored seed intent for the NOTA → schema-language → schema-rust train.

use synchronizer::release_train::{
    CandidateSelector, ReleaseTrainIntent, ReleaseTrainName, TrainComponent,
};
use synchronizer::types::{BranchName, CommitIdentifier, ComponentName};

fn main() {
    let placeholder_base = CommitIdentifier::new("0".repeat(40));
    let intent = ReleaseTrainIntent::new(
        ReleaseTrainName::new("language-family-poc"),
        vec![
            TrainComponent::new(
                ComponentName::new("nota"),
                CandidateSelector::Branch(BranchName::new("next-gen")),
                CommitIdentifier::new("18e2e8d0dba37e9e84045af3608585b51f6e3b36"),
            ),
            TrainComponent::new(
                ComponentName::new("schema-language"),
                CandidateSelector::Branch(BranchName::new("poc-structural")),
                placeholder_base.clone(),
            ),
            TrainComponent::new(
                ComponentName::new("schema-rust"),
                CandidateSelector::Mainline,
                placeholder_base,
            ),
        ],
        Vec::new(),
    );
    print!("{}", intent.to_nota_text());
}
