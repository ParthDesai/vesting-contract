#![cfg(feature = "test-bpf")]
use std::str::FromStr;

use solana_program::{hash::Hash, pubkey::Pubkey, rent::Rent, sysvar};
use solana_program::instruction::InstructionError;
use solana_program_test::{processor, ProgramTest};
use solana_sdk::{account::Account, signature::Keypair, signature::Signer, system_instruction, transaction::Transaction};
use solana_sdk::program_pack::Pack;
use solana_sdk::transaction::TransactionError;
use token_vesting::{entrypoint::process_instruction, instruction::Schedule};
use token_vesting::instruction::{unlock, change_destination, create, purge};
use spl_token::{self, instruction::{initialize_mint, initialize_account, mint_to}, state};
use token_vesting::state::{VestingSchedule, VestingScheduleHeader};

#[tokio::test]
async fn test_token_vesting() {

    // Create program and test environment
    let program_id = Pubkey::from_str("VestingbGKPFXCWuBvfkegQfZyiNwAJb9Ss623VQ5DA").unwrap();
    let mint_authority = Keypair::new();
    let mint = Keypair::new();

    let source_account = Keypair::new();
    let source_token_account = Keypair::new();

    let destination_account = Keypair::new();
    let destination_token_account = Keypair::new();

    let new_destination_account = Keypair::new();
    let new_destination_token_account = Keypair::new();

    let vesting_admin_account = Keypair::new();
    let vesting_admin_token_account = Keypair::new();

    let mut seeds = [42u8; 32];
    let (vesting_account_key, bump) = Pubkey::find_program_address(&[&seeds[..31]], &program_id);
    seeds[31] = bump;
    let vesting_token_account = Keypair::new();
    
    let mut program_test = ProgramTest::new(
        "token_vesting",
        program_id,
        processor!(process_instruction),
    );

    // Add accounts         
    program_test.add_account(
        source_account.pubkey(),
        Account {
            lamports: 5000000,
            ..Account::default()
        },
    );

    program_test.add_account(
        vesting_admin_account.pubkey(),
        Account {
            lamports: 5000000,
            ..Account::default()
        },
    );

    program_test.add_account(
        vesting_account_key,
        Account{
            lamports: 500000,
            data: vec![0; VestingScheduleHeader::LEN + VestingSchedule::LEN * 3],
            owner: program_id,
            executable: false,
            rent_epoch: 0
        }
    );

    // Start and process transactions on the test network
    let program_test_context = program_test.start_with_context().await;
    let mut banks_client = program_test_context.banks_client;
    let payer = program_test_context.payer;
    let recent_blockhash = program_test_context.last_blockhash;

    let start_time = banks_client.get_clock().await.unwrap().unix_timestamp as u64;

    // Initialize the token accounts
    banks_client.process_transaction(mint_init_transaction(
        &payer,
        &mint,
        &mint_authority,
        recent_blockhash
    )).await.unwrap();

    banks_client.process_transaction(
        create_token_account(&payer, &mint, recent_blockhash, &source_token_account, &source_account.pubkey())
    ).await.unwrap();
    banks_client.process_transaction(
        create_token_account(&payer, &mint, recent_blockhash, &vesting_token_account, &vesting_account_key)
    ).await.unwrap();
    banks_client.process_transaction(
        create_token_account(&payer, &mint, recent_blockhash, &destination_token_account, &destination_account.pubkey())
    ).await.unwrap();
    banks_client.process_transaction(
        create_token_account(&payer, &mint, recent_blockhash, &new_destination_token_account, &new_destination_account.pubkey())
    ).await.unwrap();
    banks_client.process_transaction(
        create_token_account(&payer, &mint, recent_blockhash, &vesting_admin_token_account, &vesting_admin_account.pubkey())
    ).await.unwrap();

    // Create and process the vesting transactions
    let setup_instructions = [
        mint_to(
            &spl_token::id(), 
            &mint.pubkey(), 
            &source_token_account.pubkey(), 
            &mint_authority.pubkey(), 
            &[], 
            200
        ).unwrap()
    ];

    let total_amount = 89;
    let tge_amount = 50;
    let schedules = vec![
        Schedule {amount: 13, release_time: start_time},
        Schedule {amount: 13, release_time: start_time + 60000},
        Schedule {amount: 13, release_time: start_time + 80000}
    ];
    let expected_amount_in_dest = tge_amount + schedules[0].amount;
    let expected_amount_in_admin = total_amount - expected_amount_in_dest;

    let test_instructions = [
        create(
            &program_id,
            &spl_token::id(),
            &vesting_account_key,
            &vesting_token_account.pubkey(),
            &source_account.pubkey(),
            &source_token_account.pubkey(),
            &destination_token_account.pubkey(),
            &mint.pubkey(),
            &vesting_admin_account.pubkey(),
            schedules.clone(),
            seeds.clone(),
            tge_amount,
            2
        ).unwrap(),
        unlock(
            &program_id,
            &spl_token::id(),
            &sysvar::clock::id(),
            &vesting_account_key,
            &vesting_token_account.pubkey(),
            &destination_token_account.pubkey(),
            seeds.clone()
        ).unwrap()
    ];

    let change_destination_instructions = [
        change_destination(
            &program_id,
            &vesting_account_key,
            &vesting_admin_account.pubkey(),
            &new_destination_token_account.pubkey(),
            seeds.clone()
        ).unwrap()
    ];

    let purge_instructions = [
        purge(
            &program_id,
            &spl_token::id(),
            &vesting_account_key,
            &vesting_admin_account.pubkey(),
            &vesting_token_account.pubkey(),
            &vesting_admin_token_account.pubkey(),
            seeds.clone()
        ).unwrap()
    ];
    
    // Process transaction on test network
    let mut setup_transaction = Transaction::new_with_payer(
        &setup_instructions,
        Some(&payer.pubkey()),
    );
    setup_transaction.partial_sign(
        &[
            &payer,
            &mint_authority
            ], 
        recent_blockhash
    );
    
    banks_client.process_transaction(setup_transaction).await.unwrap();

    // Try to process transaction without admin sign.
    let mut invalid_sign_instructions = [
        create(
            &program_id,
            &spl_token::id(),
            &vesting_account_key,
            &vesting_token_account.pubkey(),
            &source_account.pubkey(),
            &source_token_account.pubkey(),
            &destination_token_account.pubkey(),
            &mint.pubkey(),
            &vesting_admin_account.pubkey(),
            schedules,
            seeds.clone(),
            13,
            2
        ).unwrap(),
    ];
    invalid_sign_instructions[0].accounts.last_mut().unwrap().is_signer = false;

    let mut invalid_test_transaction = Transaction::new_with_payer(
        &invalid_sign_instructions,
        Some(&payer.pubkey()),
    );
    invalid_test_transaction.partial_sign(
        &[
            &payer,
            &source_account,
        ],
        recent_blockhash
    );

    let expected_err = banks_client.process_transaction(invalid_test_transaction).await.map_err(|e| e.unwrap()).expect_err("Transaction cannot be processed.");
    match expected_err {
        TransactionError::InstructionError(0, inner_err) => {
            if inner_err != InstructionError::MissingRequiredSignature {
                panic!("Expected Missing signature error, found: {:?}", inner_err);
            }
        }
        _ => panic!("Unexpected error")
    }

    // Process transaction on test network
    let mut test_transaction = Transaction::new_with_payer(
        &test_instructions,
        Some(&payer.pubkey()),
    );
    test_transaction.partial_sign(
        &[
            &payer,
            &source_account,
            &vesting_admin_account
            ], 
        recent_blockhash
    );
    
    banks_client.process_transaction(test_transaction).await.unwrap();

    let vesting_account = banks_client.get_account(vesting_account_key).await.unwrap().unwrap();
    let vesting_state = VestingScheduleHeader::unpack(&vesting_account.data[..VestingScheduleHeader::LEN]).unwrap();
    if vesting_state.vesting_admin != vesting_admin_account.pubkey() {
        panic!("Vesting admin is not set");
    }
    if vesting_state.destination_address != destination_token_account.pubkey() {
        panic!("Destination account is not set properly")
    }
    if vesting_state.cliff != 2 {
        panic!("Vesting state cliff is not set");
    }
    if !vesting_state.is_initialized {
        panic!("Vesting state is not initialized");
    }
    if vesting_state.is_purged {
        panic!("Vesting state is purged");
    }
    if !vesting_state.is_tge_amount_paid {
        panic!("TGE amount must be paid");
    }

    let destination_account = banks_client.get_account(destination_token_account.pubkey()).await.unwrap().unwrap();
    let destination_token_account_state = state::Account::unpack(&destination_account.data).unwrap();
    if destination_token_account_state.amount != expected_amount_in_dest {
        panic!("Destination token account does not have correct balance. Expected: {}, found: {}", expected_amount_in_dest, destination_token_account_state.amount);
    }


    // Try to process transaction without admin signature
    let mut invalid_change_destination_instruction = [
        change_destination(
            &program_id,
            &vesting_account_key,
            &vesting_admin_account.pubkey(),
            &new_destination_token_account.pubkey(),
            seeds.clone()
        ).unwrap()
    ];
    invalid_change_destination_instruction[0].accounts[1].is_signer = false;

    let mut invalid_change_destination_transaction = Transaction::new_with_payer(
        &invalid_change_destination_instruction,
        Some(&payer.pubkey())
    );

    invalid_change_destination_transaction.partial_sign(
        &[
            &payer,
        ],
        recent_blockhash
    );

    let expected_err = banks_client.process_transaction(invalid_change_destination_transaction).await.map_err(|e| e.unwrap()).expect_err("Transaction cannot be processed.");
    match expected_err {
        TransactionError::InstructionError(0, inner_err) => {
            if inner_err != InstructionError::MissingRequiredSignature {
                panic!("Expected Missing signature error, found: {:?}", inner_err);
            }
        }
        _ => panic!("Unexpected error")
    }
    
    let mut change_destination_transaction = Transaction::new_with_payer(
        &change_destination_instructions, 
        Some(&payer.pubkey())
    );

    change_destination_transaction.partial_sign(
        &[
            &payer,
            &vesting_admin_account
        ], 
        recent_blockhash
    );

    banks_client.process_transaction(change_destination_transaction).await.unwrap();

    let vesting_account = banks_client.get_account(vesting_account_key).await.unwrap().unwrap();
    let vesting_state = VestingScheduleHeader::unpack(&vesting_account.data[..VestingScheduleHeader::LEN]).unwrap();
    if vesting_state.vesting_admin != vesting_admin_account.pubkey() {
        panic!("Vesting admin is not set");
    }
    if vesting_state.destination_address != new_destination_token_account.pubkey() {
        panic!("New destination is not set properly");
    }
    if vesting_state.cliff != 2 {
        panic!("Vesting state cliff is not set");
    }
    if !vesting_state.is_initialized {
        panic!("Vesting state is not initialized");
    }
    if vesting_state.is_purged {
        panic!("Vesting state is purged");
    }

    // Try to process transaction without admin signature
    let mut invalid_purge_instructions = [
        purge(
            &program_id,
            &spl_token::id(),
            &vesting_account_key,
            &vesting_admin_account.pubkey(),
            &vesting_token_account.pubkey(),
            &vesting_admin_token_account.pubkey(),
            seeds.clone()
        ).unwrap()
    ];
    invalid_purge_instructions[0].accounts[1].is_signer = false;

    let mut invalid_purge_transaction = Transaction::new_with_payer(
        &invalid_purge_instructions,
        Some(&payer.pubkey())
    );

    invalid_purge_transaction.partial_sign(
        &[
            &payer,
        ],
        recent_blockhash
    );

    let expected_err = banks_client.process_transaction(invalid_purge_transaction).await.map_err(|e| e.unwrap()).expect_err("Transaction cannot be processed.");
    match expected_err {
        TransactionError::InstructionError(0, inner_err) => {
            if inner_err != InstructionError::MissingRequiredSignature {
                panic!("Expected Missing signature error, found: {:?}", inner_err);
            }
        }
        _ => panic!("Unexpected error")
    }

    let mut purge_transaction = Transaction::new_with_payer(
        &purge_instructions,
        Some(&payer.pubkey())
    );

    purge_transaction.partial_sign(
        &[
            &payer,
            &vesting_admin_account
        ],
        recent_blockhash
    );

    banks_client.process_transaction(purge_transaction).await.unwrap();

    let vesting_account = banks_client.get_account(vesting_account_key).await.unwrap().unwrap();
    let vesting_state = VestingScheduleHeader::unpack(&vesting_account.data[..VestingScheduleHeader::LEN]).unwrap();
    if vesting_state.vesting_admin != vesting_admin_account.pubkey() {
        panic!("Vesting admin is not set");
    }
    if vesting_state.destination_address != new_destination_token_account.pubkey() {
        panic!("New destination is not set properly");
    }
    if vesting_state.cliff != 2 {
        panic!("Vesting state cliff is not set");
    }
    if !vesting_state.is_initialized {
        panic!("Vesting state is not initialized");
    }
    if !vesting_state.is_purged {
        panic!("Vesting state is not purged");
    }

    let admin_token_account = banks_client.get_account(vesting_admin_token_account.pubkey()).await.unwrap().unwrap();
    let admin_token_account_state = state::Account::unpack(&admin_token_account.data).unwrap();
    if admin_token_account_state.amount != expected_amount_in_admin {
        panic!("Admin token account does not have correct balance. Expected: {}, found: {}", expected_amount_in_admin, admin_token_account_state.amount);
    }

    let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();

    let mut test_transaction = Transaction::new_with_payer(
        &test_instructions,
        Some(&payer.pubkey()),
    );
    test_transaction.partial_sign(
        &[
            &payer,
            &source_account,
            &vesting_admin_account
        ],
        recent_blockhash
    );

    let expected_err = banks_client.process_transaction(test_transaction).await.map_err(|e| e.unwrap()).expect_err("It needs to be errored out.");
    match expected_err {
        TransactionError::InstructionError(0, inner_err) => {
            if inner_err != InstructionError::InvalidArgument {
                panic!("Expected Invalid argument error, found: {:?}", inner_err);
            }
        }
        _ => panic!("Unexpected error")
    }


    let test_instructions = [
        unlock(
            &program_id,
            &spl_token::id(),
            &sysvar::clock::id(),
            &vesting_account_key,
            &vesting_token_account.pubkey(),
            &destination_token_account.pubkey(),
            seeds.clone()
        ).unwrap()
    ];

    let mut test_transaction = Transaction::new_with_payer(
        &test_instructions,
        Some(&payer.pubkey()),
    );
    test_transaction.partial_sign(
        &[
            &payer,
        ],
        recent_blockhash
    );

    let expected_err = banks_client.process_transaction(test_transaction).await.map_err(|e| e.unwrap()).expect_err("It needs to be errored out.");
    match expected_err {
        TransactionError::InstructionError(0, inner_err) => {
            if inner_err != InstructionError::InvalidInstructionData {
                panic!("Expected Invalid instruction data error, found: {:?}", inner_err);
            }
        }
        _ => panic!("Unexpected error")
    }

    let mut change_destination_transaction = Transaction::new_with_payer(
        &change_destination_instructions,
        Some(&payer.pubkey())
    );

    change_destination_transaction.partial_sign(
        &[
            &payer,
            &vesting_admin_account
        ],
        recent_blockhash
    );

    let expected_err = banks_client.process_transaction(change_destination_transaction).await.map_err(|e| e.unwrap()).expect_err("It needs to be errored out.");
    match expected_err {
        TransactionError::InstructionError(0, inner_err) => {
            if inner_err != InstructionError::InvalidArgument {
                panic!("Expected Invalid argument error, found: {:?}", inner_err);
            }
        }
        _ => panic!("Unexpected error")
    }

    let mut purge_transaction = Transaction::new_with_payer(
        &purge_instructions,
        Some(&payer.pubkey())
    );

    purge_transaction.partial_sign(
        &[
            &payer,
            &vesting_admin_account
        ],
        recent_blockhash
    );

    let expected_err = banks_client.process_transaction(purge_transaction).await.map_err(|e| e.unwrap()).expect_err("It needs to be errored out.");
    match expected_err {
        TransactionError::InstructionError(0, inner_err) => {
            if inner_err != InstructionError::InvalidArgument {
                panic!("Expected Invalid argument error, found: {:?}", inner_err);
            }
        }
        _ => panic!("Unexpected error")
    }
}

fn mint_init_transaction(
    payer: &Keypair, 
    mint:&Keypair, 
    mint_authority: &Keypair, 
    recent_blockhash: Hash) -> Transaction{
    let instructions = [
        system_instruction::create_account(
            &payer.pubkey(),
            &mint.pubkey(),
            Rent::default().minimum_balance(82),
            82,
            &spl_token::id()
    
        ),
        initialize_mint(
            &spl_token::id(), 
            &mint.pubkey(), 
            &mint_authority.pubkey(),
            None, 
            0
        ).unwrap(),
    ];
    let mut transaction = Transaction::new_with_payer(
        &instructions,
        Some(&payer.pubkey()),
    );
    transaction.partial_sign(
        &[
            payer,
            mint
            ], 
        recent_blockhash
    );
    transaction
}

fn create_token_account(
    payer: &Keypair, 
    mint:&Keypair, 
    recent_blockhash: Hash,
    token_account:&Keypair,
    token_account_owner: &Pubkey
) -> Transaction {
    let instructions = [
        system_instruction::create_account(
            &payer.pubkey(),
            &token_account.pubkey(),
            Rent::default().minimum_balance(165),
            165,
            &spl_token::id()
        ),
        initialize_account(
            &spl_token::id(), 
            &token_account.pubkey(), 
            &mint.pubkey(), 
            token_account_owner
        ).unwrap()
   ];
   let mut transaction = Transaction::new_with_payer(
    &instructions,
    Some(&payer.pubkey()),
    );
    transaction.partial_sign(
        &[
            payer,
            token_account
            ], 
        recent_blockhash
    );
    transaction
}